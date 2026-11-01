//! Local conversation storage. Reports, proposals and worker state are never restored.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::Read;

const LIMIT: u64 = 12 * 1024 * 1024;
const SESSIONS: usize = 40;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Session {
    pub chat: chat::Chat,
    pub mode: ChatMode,
    pub repo: Option<PathBuf>,
    pub files: String,
    pub draft: String,
}
impl Session {
    pub fn title(&self) -> String {
        self.chat
            .messages
            .iter()
            .find(|m| m.user)
            .map(|m| {
                m.text
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(36)
                    .collect()
            })
            .unwrap_or_else(|| "Новый чат".into())
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct History {
    pub sessions: Vec<Session>,
    pub active: usize,
    #[serde(skip)]
    pub dirty: Option<Instant>,
}
impl History {
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::other("История слишком велика"));
        }
        let history: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if history.sessions.len() > SESSIONS
            || (!history.sessions.is_empty() && history.active >= history.sessions.len())
            || history.sessions.iter().any(|s| {
                !s.chat.valid()
                    || s.files.len() > 32768
                    || s.draft.len() > 65536
                    || s.repo.as_ref().is_some_and(|p| p.as_os_str().len() > 4096)
            })
        {
            return Err(io::Error::other("Некорректная история чатов"));
        }
        Ok(history)
    }
    pub fn save(&mut self, path: &std::path::Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::other("История слишком велика"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Нет папки истории"))?;
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.persist(path).map_err(|e| e.error)?;
        self.dirty = None;
        Ok(())
    }
}

impl Desktop {
    pub(super) fn capture_session(&mut self) {
        if !self.real_app {
            return;
        }
        let session = Session {
            chat: self.chat.clone(),
            mode: self.chat_mode,
            repo: self.prefs.repo.clone(),
            files: self.prefs.files.clone(),
            draft: self.task.clone(),
        };
        if let Some(current) = self
            .conversations
            .sessions
            .get_mut(self.conversations.active)
        {
            *current = session;
        } else {
            self.conversations.sessions.push(session);
            self.conversations.active = self.conversations.sessions.len() - 1;
        }
        self.conversations.dirty = Some(Instant::now());
    }
    pub(super) fn open_session(&mut self, index: usize) {
        if self.job.is_some() || index >= self.conversations.sessions.len() {
            return;
        }
        self.capture_session();
        let session = self.conversations.sessions[index].clone();
        self.reset_chat();
        self.chat = session.chat;
        self.chat_mode = session.mode;
        self.prefs.repo = session.repo;
        self.prefs.files = session.files;
        self.task = session.draft;
        self.conversations.active = index;
        self.conversations.dirty = Some(Instant::now());
    }
    pub(super) fn new_session(&mut self) {
        if self.job.is_some() {
            return;
        }
        self.capture_session();
        if self
            .conversations
            .sessions
            .last()
            .is_some_and(|s| s.chat.messages.is_empty() && s.draft.is_empty())
        {
            let index = self.conversations.sessions.len() - 1;
            self.open_session(index);
            return;
        }
        self.reset_chat();
        self.chat_mode = ChatMode::Chat;
        if self.conversations.sessions.len() >= SESSIONS {
            self.conversations.sessions.remove(0);
        }
        self.conversations.sessions.push(Session::default());
        self.conversations.active = self.conversations.sessions.len() - 1;
        self.capture_session();
    }
    pub(super) fn history_sidebar(&mut self, ui: &mut egui::Ui) {
        let l = self.prefs.language;
        if ui
            .add_enabled(self.job.is_none(), egui::Button::new(l.text("Новый чат")))
            .clicked()
        {
            self.new_session();
        }
        ui.separator();
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (index, session) in self.conversations.sessions.iter().enumerate().rev() {
                if ui
                    .add_enabled(
                        self.job.is_none(),
                        egui::Button::new(session.title())
                            .selected(index == self.conversations.active)
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    )
                    .clicked()
                {
                    open = Some(index);
                }
            }
        });
        if let Some(index) = open {
            self.open_session(index);
        }
    }
    pub(super) fn save_history(&mut self, ctx: &egui::Context, force: bool) {
        if !self.history_available {
            return;
        }
        let Some(since) = self.conversations.dirty else {
            return;
        };
        if !force && since.elapsed() < Duration::from_millis(400) {
            ctx.request_repaint_after(Duration::from_millis(400));
            return;
        }
        if let Some(path) = self
            .storage
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("conversations.json"))
        {
            if let Err(error) = self.conversations.save(&path) {
                self.preference_error = Some(format!("История: {error}"));
                self.conversations.dirty = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_restores_mode_repo_and_draft_but_clears_apply_state() {
        let mut app = Desktop::default();
        app.real_app = true;
        app.prefs.repo = Some("first".into());
        app.chat_mode = ChatMode::Task;
        app.chat.sent("Fix first".into());
        app.chat.finish("Proposed".into());
        app.task = "first draft".into();
        app.capture_session();
        app.new_session();
        app.prefs.repo = Some("second".into());
        app.task = "second draft".into();
        app.report_json = b"old report".to_vec();
        app.applied.push(0);
        app.open_session(0);
        assert_eq!(app.prefs.repo, Some("first".into()));
        assert_eq!(app.task, "first draft");
        assert_eq!(app.chat_mode, ChatMode::Task);
        assert_eq!(app.chat.messages.len(), 2);
        assert!(app.report_json.is_empty());
        assert!(app.applied.is_empty());
        assert!(app.report.is_none());
        app.open_session(1);
        assert_eq!(app.prefs.repo, Some("second".into()));
        assert_eq!(app.task, "second draft");
        assert_eq!(app.chat_mode, ChatMode::Chat);
    }
    #[test]
    fn roundtrip_preserves_unicode_draft_and_repo_without_worker_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("история.json");
        let mut history = History::default();
        let mut session = Session {
            repo: Some("Проект".into()),
            mode: ChatMode::Task,
            draft: "Продолжи".into(),
            ..Default::default()
        };
        session.chat.sent("Привет\r\nмир".into());
        history.sessions.push(session);
        history.save(&path).unwrap();
        let restored = History::load(&path).unwrap();
        assert_eq!(restored.sessions[0].draft, "Продолжи");
        assert_eq!(restored.sessions[0].chat.messages[0].text, "Привет\r\nмир");
        assert!(!restored.sessions[0].chat.pending);
        assert_eq!(restored.sessions[0].mode, ChatMode::Task);
        std::fs::write(&path, br#"{"sessions":[],"active":99}"#).unwrap();
        // An empty store is legal and starts a fresh conversation.
        assert!(History::load(&path).unwrap().sessions.is_empty());
    }
}
