//! Pinned official GGUF catalog; downloading never enables cloud inference.
use super::*;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};

pub(super) struct Model {
    name: &'static str,
    repo: &'static str,
    revision: &'static str,
    file: &'static str,
    bytes: u64,
    hash: &'static str,
    ram_gib: u64,
}
const CATALOG: [Model; 2] = [
    Model {
        name: "Qwen2.5-Coder 1.5B · Q4_K_M",
        repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF",
        revision: "f86cb2c1fa58255f8052cc32aeede1b7482d4361",
        file: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
        bytes: 1117320768,
        hash: "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046",
        ram_gib: 6,
    },
    Model {
        name: "Qwen2.5-Coder 7B · Q4_K_M",
        repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
        revision: "13fb94bfda8c8cf22497dc57b78f391a9acb426a",
        file: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
        bytes: 4683073536,
        hash: "509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c",
        ram_gib: 12,
    },
];

#[derive(Default)]
pub(super) struct Installer {
    ram: Option<u64>,
    selected: usize,
    result: Option<mpsc::Receiver<io::Result<PathBuf>>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
    message: String,
}
fn recommended(ram: u64) -> usize {
    usize::from(ram >= 12 * 1024 * 1024 * 1024)
}
impl Installer {
    pub fn detect(&mut self) {
        // Memory snapshot only; no hardware profile is stored in the repository.
        if let Ok(system) = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::nothing()
                .with_memory(sysinfo::MemoryRefreshKind::nothing().with_ram()),
        ) {
            self.ram = Some(system.total_memory());
            self.selected = recommended(system.total_memory());
        }
    }
    fn start(&mut self, directory: PathBuf, ctx: egui::Context) {
        if self.result.is_some() {
            return;
        }
        let index = self.selected;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress = Arc::new(AtomicU64::new(0));
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        let (tx, rx) = mpsc::channel();
        self.result = Some(rx);
        self.message.clear();
        std::thread::spawn(move || {
            let result = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(async {
                    tokio::select! {
                        result=install(&CATALOG[index],&directory,&progress)=>result,
                        _=cancelled(cancel)=>Err(io::Error::new(io::ErrorKind::Interrupted,"Загрузка отменена")),
                    }
                })
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
impl Drop for Installer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

async fn cancelled(cancel: Arc<AtomicBool>) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn check_disk(directory: &std::path::Path, size: u64) -> io::Result<()> {
    let directory = directory.canonicalize()?;
    let disks = sysinfo::Disks::new_with_refreshed_list().map_err(io::Error::other)?;
    let disk = disks
        .iter()
        .filter(|d| directory.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .ok_or_else(|| io::Error::other("Не удалось проверить свободное место"))?;
    if disk.available_space() < size + 256 * 1024 * 1024 {
        return Err(io::Error::other("Недостаточно места для GGUF"));
    }
    Ok(())
}

fn verify(bytes: u64, magic: &[u8], hash: &str, model: &Model) -> io::Result<()> {
    if bytes != model.bytes || magic != b"GGUF" || hash != model.hash {
        return Err(io::Error::other(
            "GGUF: размер, заголовок или SHA256 не совпадает с каталогом",
        ));
    }
    Ok(())
}
async fn install(
    model: &Model,
    directory: &std::path::Path,
    progress: &AtomicU64,
) -> io::Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let target = directory.join(model.file);
    if target.exists() {
        return Err(io::Error::other(
            "Файл уже существует. Выберите его через «Модель GGUF» или другую папку.",
        ));
    }
    check_disk(directory, model.bytes)?;
    let client = reqwest::Client::builder()
        .user_agent("Lyra GGUF installer")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            let host = url.host_str().unwrap_or("");
            if attempt.previous().len() < 8
                && url.scheme() == "https"
                && (host == "huggingface.co"
                    || host.ends_with(".huggingface.co")
                    || host.ends_with(".hf.co")
                    || host.ends_with(".xethub.hf.co"))
            {
                attempt.follow()
            } else {
                attempt.error("Untrusted GGUF redirect")
            }
        }))
        .build()
        .map_err(io::Error::other)?;
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        model.repo, model.revision, model.file
    );
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| io::Error::other("Не удалось подключиться к Hugging Face"))?
        .error_for_status()
        .map_err(|_| io::Error::other("Сервер GGUF вернул ошибку"))?;
    if response
        .content_length()
        .is_some_and(|size| size != model.bytes)
    {
        return Err(io::Error::other("Неверный размер GGUF на сервере"));
    }
    receive(response, model, directory, progress).await
}
async fn receive(
    mut response: reqwest::Response,
    model: &Model,
    directory: &std::path::Path,
    progress: &AtomicU64,
) -> io::Result<PathBuf> {
    let target = directory.join(model.file);
    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut magic = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| io::Error::other("Соединение загрузки GGUF прервано"))?
    {
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| io::Error::other("GGUF size overflow"))?;
        if bytes > model.bytes {
            return Err(io::Error::other("GGUF превышает размер каталога"));
        }
        magic.extend(chunk.iter().take(4usize.saturating_sub(magic.len())));
        digest.update(&chunk);
        temp.write_all(&chunk)?;
        progress.store(bytes, Ordering::Relaxed);
    }
    verify(bytes, &magic, &format!("{:x}", digest.finalize()), model)?;
    temp.as_file_mut().sync_all()?;
    temp.persist_noclobber(&target).map_err(|e| e.error)?;
    Ok(target)
}

impl Desktop {
    pub(super) fn poll_model_install(&mut self, ctx: &egui::Context) {
        if let Some(result) = self
            .model_installer
            .result
            .as_ref()
            .and_then(|r| r.try_recv().ok())
        {
            self.model_installer.result = None;
            match result {
                Ok(path) => {
                    self.prefs.model = Some(path);
                    self.model_installer.message = "GGUF установлена и выбрана".into();
                }
                Err(error) => self.model_installer.message = error.to_string(),
            }
        }
        if self.model_installer.result.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
    pub(super) fn model_suggestion(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        ui.group(|ui| {
            ui.label(l.text("Подобрать локальную GGUF"));
            let installer = &mut self.model_installer;
            if let Some(ram) = installer.ram {
                ui.label(format!(
                    "RAM: {:.1} GiB · {}",
                    ram as f64 / 1073741824.0,
                    l.text("рекомендация для контекста 4096")
                ));
            }
            ui.add_enabled_ui(installer.result.is_none(), |ui| {
                egui::ComboBox::from_id_salt("gguf-catalog")
                    .selected_text(CATALOG[installer.selected].name)
                    .show_ui(ui, |ui| {
                        for (i, model) in CATALOG.iter().enumerate() {
                            ui.selectable_value(&mut installer.selected, i, model.name);
                        }
                    });
            });
            let model = &CATALOG[installer.selected];
            ui.label(format!(
                "{:.2} GB · RAM ≥ {} GiB · Apache-2.0",
                model.bytes as f64 / 1e9,
                model.ram_gib
            ));
            ui.hyperlink_to(
                l.text("Источник и лицензия"),
                format!("https://huggingface.co/{}", model.repo),
            );
            if installer
                .ram
                .is_some_and(|ram| ram < model.ram_gib * 1073741824)
            {
                ui.label(l.text("Для этой модели рекомендуется больше RAM"));
            }
            if installer.result.is_some() {
                ui.add(
                    egui::ProgressBar::new(
                        installer.progress.load(Ordering::Relaxed) as f32 / model.bytes as f32,
                    )
                    .show_percentage(),
                );
                if ui.button(l.text("Отмена")).clicked() {
                    installer.cancel.store(true, Ordering::Relaxed);
                }
            } else if ui
                .add_enabled(
                    self.real_app && self.job.is_none(),
                    egui::Button::new(l.text("Установить GGUF…")),
                )
                .clicked()
                && let Some(directory) = rfd::FileDialog::new()
                    .set_title(l.text("Папка для моделей GGUF"))
                    .pick_folder()
            {
                installer.start(directory, ui.ctx().clone());
            }
            if !installer.message.is_empty() {
                ui.label(l.text(&installer.message));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streamed_install_rejects_corruption_and_never_overwrites() {
        use std::io::Read;
        use std::net::TcpListener;
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"GGUFtest weights";
        let hash = format!("{:x}", Sha256::digest(bytes));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let model = Model {
            name: "fixture",
            repo: "unused",
            revision: "unused",
            file: "test.gguf",
            bytes: bytes.len() as u64,
            hash: Box::leak(hash.into_boxed_str()),
            ram_gib: 0,
        };
        for (index, corrupt) in [true, false, false].into_iter().enumerate() {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).unwrap();
                let payload = if corrupt { b"HTMLbad! weights" } else { bytes };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                )
                .unwrap();
                stream.write_all(payload).unwrap();
            });
            let result = runtime.block_on(async {
                let response = reqwest::Client::new()
                    .get(format!("http://{address}/fixture"))
                    .send()
                    .await
                    .unwrap();
                receive(response, &model, dir.path(), &AtomicU64::new(0)).await
            });
            server.join().unwrap();
            if index == 1 {
                assert_eq!(std::fs::read(result.unwrap()).unwrap(), bytes);
            } else {
                assert!(result.is_err());
            }
            if corrupt {
                assert!(!dir.path().join("test.gguf").exists());
            }
        }
        assert_eq!(std::fs::read(dir.path().join("test.gguf")).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let cancelled_flag = Arc::new(AtomicBool::new(true));
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_millis(100), cancelled(cancelled_flag))
                .await
                .unwrap();
        });
    }
    #[test]
    fn selection_and_integrity_gate() {
        assert_eq!(recommended(8 * 1073741824), 0);
        assert_eq!(recommended(32 * 1073741824), 1);
        let model = &CATALOG[0];
        assert!(verify(model.bytes, b"GGUF", model.hash, model).is_ok());
        assert!(verify(model.bytes - 1, b"GGUF", model.hash, model).is_err());
        assert!(verify(model.bytes, b"HTML", model.hash, model).is_err());
        assert!(verify(model.bytes, b"GGUF", "wrong", model).is_err());
    }
}
