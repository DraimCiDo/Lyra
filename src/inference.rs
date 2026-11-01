use clap::Args;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaChatMessage, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use std::{
    fs::File,
    io::{self, Read, Write},
    num::NonZeroU32,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Args)]
pub struct GenerateArgs {
    /// Полный путь к локальному GGUF (не URL)
    #[arg(long)]
    pub model: PathBuf,
    #[arg(long)]
    pub prompt: String,
    /// Optional ECC skill manifest (JSON or SKILL.md)
    #[arg(long)]
    pub skills: Option<PathBuf>,
    /// Explicit skill IDs to apply to this generation
    #[arg(long = "skill")]
    pub selected_skills: Vec<String>,
    /// Общее окно: prompt + шаблон + ответ
    #[arg(long, default_value_t=2048, value_parser=clap::value_parser!(u32).range(128..=32768))]
    pub context: u32,
    #[arg(long, default_value_t=256, value_parser=clap::value_parser!(u32).range(1..=8192))]
    pub max_tokens: u32,
    #[arg(long, default_value_t=default_threads(), value_parser=clap::value_parser!(u32).range(1..=256))]
    pub threads: u32,
    /// 0=CPU, 1..1022=manual CUDA layers, 1023=balanced, 1024=auto GPU
    #[arg(long, default_value_t=0, value_parser=clap::value_parser!(u32).range(0..=1024))]
    pub gpu_layers: u32,
    /// Кооперативный таймаут, включая загрузку модели
    #[arg(long, default_value_t=120, value_parser=clap::value_parser!(u64).range(1..=3600))]
    pub timeout_secs: u64,
    /// Продолжение текста без шаблона чата (для base/test моделей)
    #[arg(long)]
    pub raw: bool,
}

fn fail(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

pub fn default_threads() -> u32 {
    static DEFAULT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *DEFAULT.get_or_init(|| {
        std::thread::available_parallelism().map_or(1, |count| count.get().min(8) as u32)
    })
}

pub fn validate(args: &GenerateArgs) -> io::Result<()> {
    if !(128..=32768).contains(&args.context)
        || !(1..=8192).contains(&args.max_tokens)
        || !(1..=256).contains(&args.threads)
        || !(1..=3600).contains(&args.timeout_secs)
        || args.gpu_layers > 1024
    {
        return Err(fail("Параметры генерации вне допустимого диапазона."));
    }
    if args.prompt.trim().is_empty() || args.prompt.len() > 1024 * 1024 {
        return Err(fail("Prompt должен содержать от 1 до 1048576 байт текста."));
    }
    if args.max_tokens >= args.context {
        return Err(fail("max-tokens должен быть меньше context."));
    }
    if args.gpu_layers > 0 && !cfg!(feature = "cuda") {
        return Err(fail(
            "GPU layers требуют сборку с --features cuda; CPU fallback не выполняется.",
        ));
    }
    let mut file = File::open(&args.model)?;
    if !file.metadata()?.is_file() {
        return Err(fail("Путь модели должен указывать на обычный GGUF-файл."));
    }
    let mut header = [0u8; 8];
    file.read_exact(&mut header)?;
    if &header[..4] != b"GGUF"
        || ![2, 3].contains(&u32::from_le_bytes(
            header[4..].try_into().map_err(|_| fail("GGUF header"))?,
        ))
    {
        return Err(fail("Ожидается GGUF версии 2 или 3; файл не загружается."));
    }
    Ok(())
}

pub fn context_budget(prompt_tokens: usize, max_tokens: u32, context: u32) -> io::Result<()> {
    if prompt_tokens == 0
        || (prompt_tokens as u64)
            .checked_add(u64::from(max_tokens))
            .is_none_or(|sum| sum > u64::from(context))
    {
        return Err(fail(format!(
            "Контекст переполнен: prompt {prompt_tokens} + ответ {max_tokens} > окно {context}. Уменьшите prompt/ответ или явно увеличьте context."
        )));
    }
    Ok(())
}

// Machine-readable status events stay on stderr; generated answer text stays on stdout.
fn progress_event(value: &str) {
    let _ = writeln!(io::stderr().lock(), "LYRA_PROGRESS:{value}");
}

fn progress_tokens(count: u32) {
    let _ = writeln!(io::stderr().lock(), "LYRA_PROGRESS:tokens={count}");
}

fn check(cancelled: &AtomicBool, started: Instant, timeout: Duration) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Генерация отменена.",
        ));
    }
    if started.elapsed() >= timeout {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Истёк таймаут генерации.",
        ));
    }
    Ok(())
}

pub fn generate(args: &GenerateArgs, output: &mut impl Write) -> io::Result<()> {
    with_session(args, |session| session.complete(&args.prompt, None, output))
}

pub fn with_session<T>(
    args: &GenerateArgs,
    work: impl FnOnce(&Session<'_>) -> io::Result<T>,
) -> io::Result<T> {
    validate(args)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .map_err(|e| fail(e.to_string()))?;
    let started = Instant::now();
    let timeout = Duration::from_secs(args.timeout_secs);
    let backend = LlamaBackend::init().map_err(|e| fail(e.to_string()))?;
    let model_bytes = std::fs::metadata(&args.model)?.len();
    let free_ram = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_memory(sysinfo::MemoryRefreshKind::nothing().with_ram()),
    )
    .ok()
    .map(|system| system.available_memory())
    .filter(|available| *available > 0);
    let mut gpu_layers = args.gpu_layers;
    let mut gpu_budget = 0u64;
    if args.gpu_layers > 0 {
        let devices = llama_cpp_2::list_llama_ggml_backend_devices();
        let device = devices
            .iter()
            .filter(|device| device.backend == "CUDA")
            .max_by_key(|device| device.memory_free)
            .ok_or_else(|| {
                fail("CUDA-устройство недоступно; CPU fallback не выполняется.")
            })?;
        gpu_budget = crate::resources::gpu_weight_budget(
            device.memory_free as u64,
            args.context,
        );
        if matches!(
            args.gpu_layers,
            crate::resources::GPU_AUTO | crate::resources::GPU_BALANCED
        ) {
            let blocks = crate::resources::gguf_block_count(&args.model)?;
            gpu_layers = crate::resources::recommended_layers(
                model_bytes,
                blocks,
                device.memory_free as u64,
                args.context,
                args.gpu_layers == crate::resources::GPU_BALANCED,
            );
            if gpu_layers == 0 {
                return Err(fail(
                    "Недостаточно свободной VRAM для безопасного CUDA-offload. \
                     Освободите видеопамять, уменьшите контекст или используйте CPU.",
                ));
            }
        }
        let _ = writeln!(
            io::stderr().lock(),
            "Lyra CUDA: {} · {} слоёв на GPU · свободно {:.2} GiB VRAM · бюджет весов {:.2} GiB.",
            device.description,
            gpu_layers,
            device.memory_free as f64 / 1_073_741_824.0,
            gpu_budget as f64 / 1_073_741_824.0,
        );
    }
    crate::resources::memory_preflight(model_bytes, free_ram, gpu_budget)?;
    progress_event("load");
    let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
    let model = LlamaModel::load_from_file(&backend, &args.model, &params)
        .map_err(|e| fail(format!("Не удалось загрузить GGUF: {e}")))?;
    check(&cancelled, started, timeout)?;
    progress_event("context");
    if args.context > model.n_ctx_train() {
        return Err(fail(format!(
            "context {} превышает обученное окно модели {}.",
            args.context,
            model.n_ctx_train()
        )));
    }
    work(&Session {
        model: &model,
        backend: &backend,
        args,
        cancelled: &cancelled,
        started,
        timeout,
    })
}

#[derive(Clone, Copy)]
pub struct Session<'a> {
    model: &'a LlamaModel,
    backend: &'a LlamaBackend,
    args: &'a GenerateArgs,
    cancelled: &'a AtomicBool,
    started: Instant,
    timeout: Duration,
}

impl Session<'_> {
    pub fn complete(
        &self,
        prompt: &str,
        grammar: Option<&str>,
        output: &mut impl Write,
    ) -> io::Result<()> {
        self.complete_with_system(prompt, None, grammar, output)
    }

    pub fn complete_with_system(
        &self,
        prompt: &str,
        system: Option<&str>,
        grammar: Option<&str>,
        output: &mut impl Write,
    ) -> io::Result<()> {
        if prompt.is_empty()
            || prompt.len().saturating_add(system.map_or(0, str::len)) > 1024 * 1024
        {
            return Err(fail("Prompt вне допустимого размера."));
        }
        let Self {
            model,
            backend,
            args,
            cancelled,
            started,
            timeout,
        } = *self;
        check(cancelled, started, timeout)?;
        let prompt = if args.raw {
            system.map_or_else(|| prompt.to_owned(), |system| format!("{system}\n{prompt}"))
        } else {
            let template = model.chat_template(None).map_err(|e| {
                fail(format!(
                    "Шаблон чата недоступен: {e}. Для base-модели укажите --raw."
                ))
            })?;
            let message = LlamaChatMessage::new("user".into(), prompt.to_owned())
                .map_err(|e| fail(e.to_string()))?;
            let mut messages = Vec::new();
            if let Some(system) = system {
                messages.push(
                    LlamaChatMessage::new("system".into(), system.to_owned())
                        .map_err(|e| fail(e.to_string()))?,
                );
            }
            messages.push(message);
            model
                .apply_chat_template(&template, &messages, true)
                .map_err(|e| fail(e.to_string()))?
        };
        let vocab = model.vocab();
        let tokens = vocab.tokenize(prompt.as_bytes(), true, true);
        context_budget(tokens.len(), args.max_tokens, args.context)?;
        let context_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(args.context))
            .with_n_batch(128)
            .with_n_ubatch(128)
            .with_n_threads(args.threads as i32)
            .with_n_threads_batch(args.threads as i32)
            .with_offload_kqv(args.gpu_layers > 0);
        let mut context = model
            .new_context(backend, context_params)
            .map_err(|e| fail(e.to_string()))?;
        let mut batch = LlamaBatch::new(128, 1);
        for (chunk_index, chunk) in tokens.chunks(128).enumerate() {
            check(cancelled, started, timeout)?;
            batch.clear();
            for (i, &token) in chunk.iter().enumerate() {
                let pos = chunk_index * 128 + i;
                batch
                    .add(token, pos as i32, &[0], pos + 1 == tokens.len())
                    .map_err(|e| fail(e.to_string()))?;
            }
            context
                .decode(&mut batch)
                .map_err(|e| fail(e.to_string()))?;
        }
        progress_event("generate");
        let mut sampler = if let Some(grammar) = grammar {
            LlamaSampler::chain_simple([
                LlamaSampler::grammar(model, grammar, "root").map_err(|e| fail(e.to_string()))?,
                LlamaSampler::greedy(),
            ])
        } else {
            LlamaSampler::greedy()
        };
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut count = 0;
        for index in 0..args.max_tokens {
            check(cancelled, started, timeout)?;
            let token = sampler.sample(&context, -1);
            if vocab.is_eog(token) {
                break;
            }
            // sample() already accepts the token, including grammar state.
            let bytes = vocab.token_to_piece(token, false, None);
            let mut text = String::with_capacity(bytes.len() * 3 + 16);
            let (_, read, _) = decoder.decode_to_string(&bytes, &mut text, false);
            if read != bytes.len() {
                return Err(fail("Не удалось декодировать текст токена."));
            }
            output.write_all(text.as_bytes())?;
            output.flush()?;
            count += 1;
            if count == 1 || count % 8 == 0 {
                progress_tokens(count);
            }
            if index + 1 < args.max_tokens {
                batch.clear();
                batch
                    .add(token, (tokens.len() + index as usize) as i32, &[0], true)
                    .map_err(|e| fail(e.to_string()))?;
                context
                    .decode(&mut batch)
                    .map_err(|e| fail(e.to_string()))?;
            }
        }
        progress_tokens(count);
        let mut tail = String::with_capacity(16);
        let (result, _, _) = decoder.decode_to_string(&[], &mut tail, true);
        if result != encoding_rs::CoderResult::InputEmpty {
            return Err(fail("Не удалось завершить UTF-8 поток."));
        }
        output.write_all(tail.as_bytes())?;
        writeln!(output)?;
        output.flush()?;
        let _ = writeln!(
            io::stderr().lock(),
            "Сгенерировано {count} токенов; {:.2} с с загрузкой; лимит ответа {}.",
            started.elapsed().as_secs_f64(),
            args.max_tokens
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_worker_threads_respect_available_parallelism() {
        let threads = default_threads();
        assert!((1..=8).contains(&threads));
        if let Ok(available) = std::thread::available_parallelism() {
            assert!(threads as usize <= available.get());
        }
    }
    #[test]
    fn context_reserves_the_whole_answer() {
        assert!(context_budget(100, 28, 128).is_ok());
        assert!(context_budget(101, 28, 128).is_err());
        assert!(context_budget(0, 1, 128).is_err());
        assert!(context_budget(usize::MAX, u32::MAX, 128).is_err());
    }
    #[test]
    fn cancellation_and_deadline_have_distinct_errors() {
        assert_eq!(
            check(
                &AtomicBool::new(true),
                Instant::now(),
                Duration::from_secs(10)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(
            check(&AtomicBool::new(false), Instant::now(), Duration::ZERO)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
    }
}
