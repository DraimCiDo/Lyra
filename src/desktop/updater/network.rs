use super::package::*;
use reqwest::Client;
use semver::Version;
use serde::Deserialize;
use std::{
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const API: &str = "https://api.github.com/repos/DraimCiDo/Lyra/releases?per_page=30";
const ROOT: &str = "https://github.com/DraimCiDo/Lyra/releases/download/";
#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub zip: String,
    pub sums: String,
    pub size: u64,
}
#[derive(Deserialize)]
struct RemoteRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}
pub fn select_release(
    json: &[u8],
    current: &Version,
    preview: bool,
) -> io::Result<Option<Release>> {
    let releases: Vec<RemoteRelease> = serde_json::from_slice(json).map_err(error)?;
    if releases.len() > 30 {
        return Err(error("Too many releases"));
    }
    let mut result: Option<Release> = None;
    for r in releases {
        if r.draft || r.prerelease && !preview || r.tag_name.len() > 129 {
            continue;
        }
        let Some(tag) = r.tag_name.strip_prefix('v') else {
            continue;
        };
        let Ok(version) = Version::parse(tag) else {
            continue;
        };
        if version.to_string() != tag
            || r.prerelease == version.pre.is_empty()
            || version.cmp_precedence(current) != std::cmp::Ordering::Greater
        {
            continue;
        }
        let pick = |name: &str| {
            let v: Vec<_> = r
                .assets
                .iter()
                .filter(|a| {
                    a.name == name
                        && a.browser_download_url == format!("{ROOT}{}/{name}", r.tag_name)
                })
                .collect();
            if v.len() == 1 { Some(v[0]) } else { None }
        };
        let (Some(zip), Some(sums)) = (pick(ASSET), pick(SUMS)) else {
            continue;
        };
        if zip.size == 0 || zip.size > ARCHIVE_LIMIT || sums.size == 0 || sums.size > 65536 {
            continue;
        }
        if result
            .as_ref()
            .is_none_or(|old| version.cmp_precedence(&old.version) == std::cmp::Ordering::Greater)
        {
            result = Some(Release {
                version,
                zip: zip.browser_download_url.clone(),
                sums: sums.browser_download_url.clone(),
                size: zip.size,
            });
        }
    }
    Ok(result)
}
fn client() -> io::Result<Client> {
    Client::builder()
        .user_agent(concat!("Lyra/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let u = attempt.url();
            if attempt.previous().len() > 5
                || u.scheme() != "https"
                || !matches!(
                    u.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                            | "github-releases.githubusercontent.com"
                    )
                )
            {
                attempt.error("Unexpected update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(error)
}
async fn bytes(client: &Client, url: &str, limit: usize) -> io::Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(error)?
        .error_for_status()
        .map_err(error)?;
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(error("Update metadata is too large"));
    }
    let mut b = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(error)? {
        if b.len() + chunk.len() > limit {
            return Err(error("Update metadata exceeds limit"));
        }
        b.extend_from_slice(&chunk);
    }
    Ok(b)
}
pub async fn check(preview: bool) -> io::Result<Option<Release>> {
    let c = client()?;
    let json = tokio::time::timeout(Duration::from_secs(15), bytes(&c, API, 1024 * 1024))
        .await
        .map_err(error)??;
    select_release(
        &json,
        &Version::parse(env!("CARGO_PKG_VERSION")).map_err(error)?,
        preview,
    )
}
async fn archive(
    client: &Client,
    r: &Release,
    path: &Path,
    progress: Arc<AtomicU64>,
) -> io::Result<()> {
    let mut response = client
        .get(&r.zip)
        .send()
        .await
        .map_err(error)?
        .error_for_status()
        .map_err(error)?;
    if response
        .content_length()
        .is_some_and(|n| n > ARCHIVE_LIMIT || n != r.size)
    {
        return Err(error("Unexpected archive size"));
    }
    let mut f = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await?;
    let mut count = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(error)? {
        count += chunk.len() as u64;
        if count > ARCHIVE_LIMIT || count > r.size {
            return Err(error("Archive exceeds size limit"));
        }
        f.write_all(&chunk).await?;
        progress.store(count, Ordering::Relaxed);
    }
    f.flush().await?;
    if count != r.size {
        return Err(error("Incomplete archive"));
    }
    Ok(())
}
pub async fn download(
    r: Release,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
) -> io::Result<Package> {
    let c = client()?;
    let dir = tempfile::Builder::new()
        .prefix("lyra-download-")
        .tempdir()?;
    let sums = tokio::time::timeout(Duration::from_secs(15), bytes(&c, &r.sums, 65536))
        .await
        .map_err(error)??;
    let hash = expected_hash(&sums)?;
    archive(&c, &r, &dir.path().join("package.zip"), progress).await?;
    cancelled(&cancel)?;
    unpack(dir, &r.version, &hash, &cancel)
}
pub async fn cancel_signal(flag: Arc<AtomicBool>) {
    while !flag.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
