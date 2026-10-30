//! Only catalog URLs; no model-selected URLs, search engine, credentials or project uploads.
use super::*;
use reqwest::Url;
use std::time::Duration;
const HTML_LIMIT: usize = 1024 * 1024;
pub fn allowed(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "doc.rust-lang.org"
                    | "vuejs.org"
                    | "vite.dev"
                    | "docs.python.org"
                    | "www.typescriptlang.org"
                    | "developer.mozilla.org"
                    | "learn.microsoft.com"
                    | "dev.java"
                    | "kotlinlang.org"
                    | "docs.papermc.io"
                    | "jd.papermc.io"
                    | "www.spigotmc.org"
                    | "hub.spigotmc.org"
                    | "community.bistudio.com"
            )
        )
}
pub fn fetch_blocking(profile: Profile, version: &str) -> io::Result<Pack> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(90), fetch(profile, version))
                .await
                .map_err(err)?
        })
}
async fn fetch(profile: Profile, version: &str) -> io::Result<Pack> {
    let locations = sources(profile, version)?;
    if locations.is_empty() {
        return Err(err(
            "Для общего профиля импортируй документацию выбранного стека локально.",
        ));
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("Lyra/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.previous().len() > 3 || !allowed(a.url()) {
                a.error("Documentation redirect is not allowed")
            } else {
                a.follow()
            }
        }))
        .build()
        .map_err(err)?;
    let mut pack = Pack {
        schema_version: 1,
        profile,
        target_version: version.into(),
        documents: Vec::new(),
    };
    for location in locations {
        let url = Url::parse(&location.url).map_err(err)?;
        if !allowed(&url) {
            return Err(err("Источник отсутствует в allowlist."));
        }
        let mut response = client
            .get(url)
            .send()
            .await
            .map_err(err)?
            .error_for_status()
            .map_err(err)?;
        let final_url = response.url().clone();
        if !allowed(&final_url) {
            return Err(err("Unexpected documentation URL"));
        }
        let kind = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !kind.starts_with("text/html") && !kind.starts_with("text/plain") {
            return Err(err("Ожидается HTML или plain text документации."));
        }
        if response
            .content_length()
            .is_some_and(|n| n > HTML_LIMIT as u64)
        {
            return Err(err("Страница документации превышает 1 MiB."));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(err)? {
            if body.len() + chunk.len() > HTML_LIMIT {
                return Err(err("Страница превышает лимит."));
            }
            body.extend_from_slice(&chunk);
        }
        let text = if kind.starts_with("text/html") {
            html2text::from_read(body.as_slice(), 100).map_err(err)?
        } else {
            String::from_utf8(body).map_err(err)?
        };
        let text = text.trim().to_owned();
        if text.len() < 100 || text.len() > DOCUMENT_LIMIT {
            return Err(err(format!(
                "Документ пуст/слишком большой: {}",
                location.title
            )));
        }
        // Challenge/error pages must not be silently recorded as successful research.
        let lower = text.to_lowercase();
        if [
            "verify you are human",
            "checking your browser",
            "just a moment...",
            "access denied",
        ]
        .iter()
        .any(|s| lower.contains(s))
        {
            return Err(err(
                "Источник заблокирован. Импортируй локальную документацию/SDK.",
            ));
        }
        pack.documents.push(Document {
            source: Source {
                id: location.id,
                title: location.title,
                origin: final_url.to_string(),
                version_scope: location.version_scope,
                sha256: digest(text.as_bytes()),
                retrieved_unix: now(),
            },
            text,
        });
    }
    pack.validate()?;
    Ok(pack)
}
