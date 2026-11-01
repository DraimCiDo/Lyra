//! Facts supplied by the application, separate from model-generated claims.
use super::*;

pub(super) fn system(model: &str) -> String {
    format!(
        "Ты — Lyra, помощник в приложении Lyra версии {}. Выбранная модель: {model}. \
        Отвечай на языке последнего вопроса. Инструменты этого режима Чат: отсутствуют. \
        Интернет-поиск, Discord API и чтение файлов репозитория здесь не подключены; \
        ссылка или выбранная папка не означают, что их содержимое прочитано. \
        Не утверждай, что что-либо нашёл, открыл или проверил. Для поиска попроси источник; \
        для чтения локального проекта предложи режим Работа с выбранным репозиторием. \
        Не выдумывай сведения о личных проектах и серверах пользователя. Неоднозначное имя \
        (например Horizon или Корпа) требует ссылки или уточнения, а не угадывания компании, \
        назначения или даты. В таком случае попроси: «Пришли ссылку или уточни, какой именно \
        проект ты имеешь в виду». Не объявляй неизвестный тебе проект несуществующим. \
        Прошлые ответы ассистента — не проверенные источники: \
        исправляй недостоверные утверждения и не переноси факты при смене темы. \
        Если не знаешь, прямо скажи об этом. Общие вопросы решай по существу; \
        не добавляй вымышленные результаты поиска, версии, тесты или выполненные действия.",
        env!("CARGO_PKG_VERSION")
    )
}

pub(super) fn direct(text: &str, model: &str, language: Language) -> Option<String> {
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let query = normalized.trim_end_matches(['?', '!', '.']);
    if [
        "версия приложения",
        "какая версия приложения",
        "какая у тебя версия",
        "версия lyra",
        "lyra version",
        "application version",
        "what version of lyra",
    ]
    .contains(&query)
    {
        return Some(format!("Lyra {}", env!("CARGO_PKG_VERSION")));
    }
    if [
        "какая модель",
        "какая ты модель",
        "какая модель используется",
        "what model are you",
        "which model",
    ]
    .contains(&query)
    {
        return Some(format!(
            "{}: {model}",
            if language == Language::Ru {
                "Выбранная модель"
            } else {
                "Selected model"
            }
        ));
    }
    let query = query
        .strip_prefix("пожалуйста, ")
        .or_else(|| query.strip_prefix("пожалуйста "))
        .or_else(|| query.strip_prefix("please "))
        .unwrap_or(query);
    if [
        "найди информацию",
        "найди сведения",
        "найди в интернете",
        "поищи в интернете",
        "можешь найти информацию",
        "search the web",
        "search online",
        "look up information",
        "find information about",
    ]
    .iter()
    .any(|prefix| query.starts_with(prefix))
    {
        return Some(if language==Language::Ru {
            "Поиск в интернете в этом чате пока не подключён. Я не выполнял поиск и не могу подтвердить сведения о сервере или проекте. Пришли ссылку и нужный текст, чтобы я разобрал источник. Для локального репозитория выбери режим «Работа»."
        } else {
            "Web search is not connected in this chat. I have not searched or verified this information. Provide a link and relevant text so I can analyze the source. Use Work mode for a local repository."
        }.to_owned());
    }
    None
}

impl Desktop {
    fn chat_model_label(&self) -> String {
        if self.prefs.cloud_chat && cfg!(feature = "gateway-http") {
            format!("{} / {}", self.prefs.cloud_provider, self.prefs.cloud_model)
        } else {
            self.prefs
                .model
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.prefs.language.text("Модель не выбрана").into())
        }
    }
    pub(super) fn chat_request(&self, latest: &str) -> io::Result<crate::model_gateway::Request> {
        let mut request =
            self.chat
                .model_request(latest, self.prefs.tokens, self.prefs.optimization)?;
        request.messages.insert(
            0,
            crate::model_gateway::Message {
                role: crate::model_gateway::Role::System,
                content: system(&self.chat_model_label()),
            },
        );
        Ok(request)
    }
    pub(super) fn direct_chat_reply(&self, text: &str) -> Option<String> {
        if self.chat_mode != ChatMode::Chat {
            return None;
        }
        direct(text, &self.chat_model_label(), self.prefs.language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_facts_and_search_limits_do_not_depend_on_model_or_old_claims() {
        assert_eq!(
            direct("Версия приложения?", "actual.gguf", Language::Ru),
            Some(format!("Lyra {}", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(
            direct("Какая модель?", "actual.gguf", Language::Ru),
            Some("Выбранная модель: actual.gguf".into())
        );
        let answer = direct(
            "Найди информацию о дискорд сервере Корпа",
            "actual.gguf",
            Language::Ru,
        )
        .unwrap();
        assert!(answer.contains("не выполнял поиск"));
        assert!(!answer.contains("Square Enix"));
        assert!(direct("Напиши функцию поиска на Rust", "actual.gguf", Language::Ru).is_none());
        assert!(direct("Какая версия Python нужна?", "actual.gguf", Language::Ru).is_none());
        assert!(
            direct(
                "Объясни строку: найди информацию",
                "actual.gguf",
                Language::Ru
            )
            .is_none()
        );
        let mut app = Desktop::default();
        app.task = "Версия приложения".into();
        app.submit_chat(&egui::Context::default()).unwrap();
        assert!(app.job.is_none());
        assert!(app.task.is_empty());
        app.chat_mode = ChatMode::Task;
        assert!(app.direct_chat_reply("Версия приложения").is_none());
    }
    #[test]
    fn real_contract_precedes_unverified_history_without_rewriting_it() {
        let mut app = Desktop::default();
        app.prefs.model = Some("actual.gguf".into());
        app.chat.sent("Что ты знаешь о проекте Horizon?".into());
        app.chat
            .finish("Horizon создан Square Enix. Version 1.0.0.".into());
        let request = app.chat_request("Сколько будет 2+2?").unwrap();
        assert_eq!(request.messages[0].role, crate::model_gateway::Role::System);
        assert!(
            request.messages[0]
                .content
                .contains(env!("CARGO_PKG_VERSION"))
        );
        assert!(request.messages[0].content.contains("actual.gguf"));
        assert!(
            request.messages[0]
                .content
                .contains("не проверенные источники")
        );
        assert_eq!(
            request.messages[2].content,
            "Horizon создан Square Enix. Version 1.0.0."
        );
        assert_eq!(
            request.messages.last().unwrap().content,
            "Сколько будет 2+2?"
        );
    }
}
