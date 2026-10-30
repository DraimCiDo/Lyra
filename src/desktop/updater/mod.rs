pub mod install;
pub mod network;
pub mod package;
use eframe::egui;
use network::Release;
use package::{Package, error};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Check,
    Download,
    Prepare,
}
enum Event {
    Checked(Option<Release>),
    Downloaded(Package),
    Prepared,
}
struct Work {
    kind: Kind,
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<io::Result<Event>>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Work {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
pub struct Controller {
    pub available: Option<Release>,
    pub ready: Option<Package>,
    pub message: String,
    pub error: Option<String>,
    pub progress: Arc<AtomicU64>,
    pub cache: Option<PathBuf>,
    pub close_now: bool,
    work: Option<Work>,
    next_check: Instant,
    last_alert: Option<String>,
    pub attention: Option<Instant>,
    startup_ack: Option<PathBuf>,
    startup_done: bool,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            available: None,
            ready: None,
            message: "Обновления ещё не проверялись".into(),
            error: None,
            progress: Arc::new(AtomicU64::new(0)),
            cache: None,
            close_now: false,
            work: None,
            next_check: Instant::now(),
            last_alert: None,
            attention: None,
            startup_ack: None,
            startup_done: false,
        }
    }
}
impl Controller {
    pub fn supported() -> bool {
        cfg!(all(windows, target_arch = "x86_64", not(feature = "cuda")))
    }
    pub fn configure(&mut self, cache: Option<PathBuf>) {
        self.cache = cache;
        if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--updated")) {
            self.startup_ack = std::env::args_os().nth(2).map(PathBuf::from);
        }
        if let Some(cache) = &self.cache
            && let Some(r) = install::read_receipt(cache)
        {
            self.message = if r.success {
                "Обновление установлено".into()
            } else {
                "Предыдущее обновление не выполнено".into()
            };
            if !r.success {
                self.error = Some(r.message);
            }
        }
    }
    pub fn busy(&self) -> bool {
        self.work.is_some()
    }
    pub fn kind(&self) -> Option<Kind> {
        self.work.as_ref().map(|w| w.kind)
    }
    pub fn cancel(&self) {
        if let Some(w) = &self.work
            && w.kind != Kind::Prepare
        {
            w.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn fail(&mut self, e: impl std::fmt::Display) {
        self.message = "Обновление не выполнено".into();
        self.error = Some(e.to_string());
    }
    fn spawn<F, Fut>(&mut self, kind: Kind, ctx: egui::Context, operation: F) -> io::Result<()>
    where
        F: FnOnce(Arc<AtomicBool>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = io::Result<Event>> + 'static,
    {
        if self.busy() {
            return Err(error("Updater is already busy"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let cache = self.cache.clone();
        let thread=thread::Builder::new().name("lyra-updater".into()).spawn(move||{
   let result=(||{
    if let Some(cache)=cache{install::cleanup(&cache);}
    let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime.block_on(async move{
     if kind==Kind::Prepare{operation(flag).await}else{
      tokio::select!{r=operation(flag.clone())=>r,_=network::cancel_signal(flag)=>Err(io::Error::new(io::ErrorKind::Interrupted,"Update cancelled"))}
     }
    })
   })();let _=sender.send(result);ctx.request_repaint();
  })?;
        self.error = None;
        self.work = Some(Work {
            kind,
            cancel,
            receiver,
            thread: Some(thread),
        });
        Ok(())
    }
    pub fn check(&mut self, preview: bool, ctx: &egui::Context) {
        self.next_check = Instant::now() + Duration::from_secs(300);
        self.message = "Проверяю GitHub Releases…".into();
        if let Err(e) = self.spawn(Kind::Check, ctx.clone(), move |_| async move {
            network::check(preview).await.map(Event::Checked)
        }) {
            self.fail(e);
        }
    }
    pub fn download(&mut self, ctx: &egui::Context) {
        let Some(release) = self.available.clone() else {
            return;
        };
        self.progress.store(0, Ordering::Relaxed);
        let progress = self.progress.clone();
        self.message = "Скачивание и проверка…".into();
        if let Err(e) = self.spawn(Kind::Download, ctx.clone(), move |cancel| async move {
            tokio::time::timeout(
                Duration::from_secs(600),
                network::download(release, cancel, progress),
            )
            .await
            .map_err(error)?
            .map(Event::Downloaded)
        }) {
            self.fail(e);
        }
    }
    pub fn prepare(&mut self, ctx: &egui::Context) {
        let (Some(package), Some(cache)) = (self.ready.take(), self.cache.clone()) else {
            return;
        };
        self.message = "Готовлю перезапуск…".into();
        if let Err(e) = self.spawn(Kind::Prepare, ctx.clone(), move |_| async move {
            install::prepare_for_current(package, &cache).map(|_| Event::Prepared)
        }) {
            self.fail(e);
        }
    }
    pub fn poll(&mut self) {
        let result = match self.work.as_ref().map(|w| w.receiver.try_recv()) {
            Some(Ok(r)) => Some(r),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Some(Err(error("Updater worker stopped")))
            }
            _ => None,
        };
        let Some(r) = result else {
            return;
        };
        self.work = None;
        match r {
            Ok(Event::Checked(release)) => {
                self.available = release;
                if let Some(r) = &self.available {
                    self.message = "Доступна новая версия".into();
                    let tag = r.version.to_string();
                    if self.last_alert.as_ref() != Some(&tag) {
                        self.last_alert = Some(tag);
                        self.attention = Some(Instant::now());
                    }
                } else {
                    self.message = "Новых опубликованных версий пока нет".into();
                    self.attention = None;
                }
            }
            Ok(Event::Downloaded(package)) => {
                self.ready = Some(package);
                self.message = "Пакет проверен. Можно установить".into();
            }
            Ok(Event::Prepared) => {
                self.close_now = true;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                self.message = "Загрузка или проверка отменена".into();
            }
            Err(e) => self.fail(e),
        }
    }
    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        auto: bool,
        preview: bool,
        visible: bool,
        real: bool,
    ) {
        self.poll();
        if real && visible && auto && Self::supported() && !self.busy() && self.ready.is_none() {
            if Instant::now() >= self.next_check {
                self.check(preview, ctx);
            } else {
                ctx.request_repaint_after(
                    self.next_check.saturating_duration_since(Instant::now()),
                );
            }
        }
        if visible && self.busy() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
    pub fn acknowledge_startup(&mut self) {
        if self.startup_done {
            return;
        }
        self.startup_done = true;
        if let (Some(cache), Some(dir)) = (&self.cache, &self.startup_ack) {
            match install::acknowledge(cache, dir) {
                Ok(()) => self.message = "Обновление установлено".into(),
                Err(e) => self.fail(e),
            }
        }
    }
    pub fn downloaded(&self) -> u64 {
        self.progress.load(Ordering::Relaxed)
    }
}
pub fn helper_main() -> std::process::ExitCode {
    use std::io::Write;
    let quiet = std::env::args_os().any(|a| a == "--quiet");
    let path = std::env::args_os().nth(2).map(PathBuf::from);
    let result = path
        .as_deref()
        .ok_or_else(|| error("Missing helper plan"))
        .and_then(install::helper);
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            let _ = writeln!(io::stderr().lock(), "Lyra updater: {e}");
            if !quiet
                && let Some(path) = path
                && path.parent().is_some_and(|p| p.join("ready").exists())
            {
                rfd::MessageDialog::new()
                    .set_title("Lyra update")
                    .set_description(e.to_string())
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
            std::process::ExitCode::FAILURE
        }
    }
}
