//! Self-update from GitHub releases (same model as GitKwaken).
//!
//! Channel = `update.json` attached to the latest GitHub release:
//! ```json
//! { "version": "0.2.0", "notes": "…", "url": "Kwalendar_0.2.0_x64-setup.exe",
//!   "sha256": "<64 hex>", "published": "2026-10-30" }
//! ```
//! `url` may be relative to the manifest. Older versions are listed through the GitHub API; a
//! release titled "… (withdrawn)" (see `.github/workflows/withdraw.yml`) is never offered, and
//! installations running it are offered a rollback to the newest good release.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Built-in channel; release builds may override it with `KW_UPDATE_CHANNEL` at build time.
pub const DEFAULT_CHANNEL: &str = match option_env!("KW_UPDATE_CHANNEL") {
    Some(c) => c,
    None => "https://github.com/maxpowerart/Kwalendar/releases/latest/download/update.json",
};

pub const WITHDRAWN_MARK: &str = "(withdrawn)";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub published: Option<String>,
    /// Where the manifest came from (to resolve a relative `url`). Filled in by us.
    #[serde(default)]
    pub manifest_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub version: String,
    pub published: String,
    pub prerelease: bool,
    pub withdrawn: bool,
    pub reason: String,
    pub manifest_url: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    /// A newer release to offer (not skipped, unless the check was manual).
    pub available: Option<Release>,
    /// The running version was withdrawn.
    pub withdrawn: Option<Withdrawn>,
    pub checked_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Withdrawn {
    pub reason: String,
    /// Newest good release to go back (or forward) to.
    pub target: Option<Listed>,
}

#[derive(Debug, Clone)]
pub struct Channel {
    pub manifest: String,
    /// GitHub API release list; `None` for non-GitHub channels (no version picker then).
    pub api: Option<String>,
}

impl Channel {
    pub fn from_env() -> Self {
        let manifest = std::env::var("KW_UPDATE_URL").unwrap_or_else(|_| DEFAULT_CHANNEL.to_string());
        let api = std::env::var("KW_UPDATE_API").ok().or_else(|| releases_api_url(&manifest));
        Self { manifest, api }
    }
}

pub fn parse_version(v: &str) -> Result<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches(['v', 'V'])).with_context(|| format!("bad version {v}"))
}

pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse_version(current), parse_version(candidate)) {
        (Ok(c), Ok(n)) => n > c,
        _ => false,
    }
}

/// `https://github.com/<owner>/<repo>/releases/…` → GitHub API list URL.
pub fn releases_api_url(channel: &str) -> Option<String> {
    let rest = channel.strip_prefix("https://github.com/")?;
    let mut p = rest.split('/');
    let (owner, repo) = (p.next()?, p.next()?);
    if owner.is_empty() || repo.is_empty() || p.next() != Some("releases") {
        return None;
    }
    Some(format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=50"))
}

pub fn resolve_url(manifest_url: &str, url: &str) -> String {
    if url.contains("://") {
        return url.to_string();
    }
    let base = manifest_url.rsplit_once('/').map_or(manifest_url, |(b, _)| b);
    format!("{base}/{}", url.trim_start_matches("./"))
}

fn withdrawn_reason(body: &str) -> String {
    body.lines()
        .map(|l| l.trim().replace("**", ""))
        .find_map(|l| l.strip_prefix("Withdrawn:").map(|r| r.trim().to_string()))
        .unwrap_or_default()
}

/// GitHub API release list → installable releases, newest first.
pub fn parse_release_list(json: &str) -> Result<Vec<Listed>> {
    let v: serde_json::Value = serde_json::from_str(json).context("invalid release list")?;
    let items = v.as_array().ok_or_else(|| anyhow!("invalid release list"))?;
    let mut out: Vec<Listed> = items
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            parse_version(tag).ok()?;
            let manifest_url = r["assets"]
                .as_array()?
                .iter()
                .find(|a| a["name"] == "update.json")?["browser_download_url"]
                .as_str()?
                .to_string();
            Some(Listed {
                version: tag.trim_start_matches(['v', 'V']).to_string(),
                published: r["published_at"].as_str().unwrap_or("").chars().take(10).collect(),
                prerelease: r["prerelease"].as_bool().unwrap_or(false),
                withdrawn: r["name"].as_str().is_some_and(|n| n.to_lowercase().contains(WITHDRAWN_MARK)),
                reason: withdrawn_reason(r["body"].as_str().unwrap_or("")),
                manifest_url,
            })
        })
        .collect();
    out.sort_by(|a, b| match (parse_version(&a.version), parse_version(&b.version)) {
        (Ok(x), Ok(y)) => y.cmp(&x),
        _ => std::cmp::Ordering::Equal,
    });
    Ok(out)
}

pub async fn fetch_manifest(http: &reqwest::Client, url: &str) -> Result<Release> {
    let resp = http.get(url).send().await?.error_for_status()?;
    let mut r: Release = serde_json::from_str(&resp.text().await?).context("invalid update.json")?;
    parse_version(&r.version)?;
    r.version = r.version.trim_start_matches(['v', 'V']).to_string();
    r.manifest_url = url.to_string();
    Ok(r)
}

pub async fn list_releases(http: &reqwest::Client, ch: &Channel) -> Result<Vec<Listed>> {
    let api = ch.api.as_deref().ok_or_else(|| anyhow!(crate::tr!("канал обновлений не на GitHub — список версий недоступен", "the update channel is not on GitHub — the version list is unavailable")))?;
    let resp = http
        .get(api)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;
    parse_release_list(&resp.text().await?)
}

/// Background or manual check. A skipped version is offered only on a manual check.
pub async fn check(http: &reqwest::Client, ch: &Channel, current: &str, skipped: &str, manual: bool) -> UpdateInfo {
    let mut info = UpdateInfo { current: current.to_string(), checked_at: Some(chrono::Utc::now().timestamp_millis()), ..Default::default() };
    match fetch_manifest(http, &ch.manifest).await {
        Ok(r) if is_newer(current, &r.version) && (manual || r.version != skipped) => info.available = Some(r),
        Ok(_) => {}
        Err(e) => info.error = Some(e.to_string()),
    }
    if ch.api.is_some() {
        if let Ok(list) = list_releases(http, ch).await {
            if let Some(me) = list.iter().find(|l| l.version == current.trim_start_matches('v') && l.withdrawn) {
                let target = list.iter().find(|l| !l.withdrawn && !l.prerelease && l.version != me.version).cloned();
                info.withdrawn = Some(Withdrawn { reason: me.reason.clone(), target });
            }
        }
    }
    info
}

/// Downloads the installer of `rel` into `dir`, verifying its SHA-256.
pub async fn download(
    http: &reqwest::Client,
    rel: &Release,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    if rel.sha256.len() != 64 {
        bail!(crate::tr!("в update.json нет SHA-256 — установка отменена", "update.json has no SHA-256 — installation cancelled"));
    }
    let url = resolve_url(&rel.manifest_url, &rel.url);
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(format!("Kwalendar_{}_x64-setup.exe", rel.version));
    let tmp = dest.with_extension("part");
    let mut resp = http
        .get(&url)
        .timeout(std::time::Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        use std::io::Write;
        file.write_all(&chunk)?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        progress(done, total);
    }
    drop(file);
    let got = format!("{:x}", hasher.finalize());
    if !got.eq_ignore_ascii_case(rel.sha256.trim()) {
        let _ = std::fs::remove_file(&tmp);
        bail!(crate::tr!("контрольная сумма не совпала — файл повреждён или подменён", "checksum mismatch — the file is damaged or was replaced"));
    }
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
      {"tag_name":"v0.3.0","name":"Kwalendar 0.3.0 (withdrawn)","draft":false,"prerelease":true,"published_at":"2026-11-10T10:00:00Z",
       "body":"**Withdrawn:** ломает синхронизацию\n\nnotes","assets":[{"name":"update.json","browser_download_url":"https://x/0.3.0/update.json"}]},
      {"tag_name":"v0.2.1","name":"Kwalendar 0.2.1","draft":false,"prerelease":false,"published_at":"2026-11-01T10:00:00Z",
       "body":"fixes","assets":[{"name":"update.json","browser_download_url":"https://x/0.2.1/update.json"}]},
      {"tag_name":"v0.4.0","name":"draft","draft":true,"assets":[{"name":"update.json","browser_download_url":"https://x/d"}]},
      {"tag_name":"v0.1.0","name":"Kwalendar 0.1.0","draft":false,"prerelease":false,"published_at":"2026-10-22T10:00:00Z",
       "body":"","assets":[{"name":"update.json","browser_download_url":"https://x/0.1.0/update.json"}]},
      {"tag_name":"nightly","name":"n","draft":false,"assets":[]}
    ]"#;

    #[test]
    fn lists_releases_newest_first_with_withdrawn_reason() {
        let l = parse_release_list(LIST).unwrap();
        assert_eq!(l.iter().map(|x| x.version.as_str()).collect::<Vec<_>>(), ["0.3.0", "0.2.1", "0.1.0"]);
        assert!(l[0].withdrawn && l[0].prerelease);
        assert_eq!(l[0].reason, "ломает синхронизацию");
        assert!(!l[1].withdrawn);
    }

    #[test]
    fn versions_and_urls() {
        assert!(is_newer("0.1.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "garbage"));
        assert_eq!(
            releases_api_url("https://github.com/maxpowerart/Kwalendar/releases/latest/download/update.json").as_deref(),
            Some("https://api.github.com/repos/maxpowerart/Kwalendar/releases?per_page=50")
        );
        assert_eq!(resolve_url("https://h/a/b/update.json", "setup.exe"), "https://h/a/b/setup.exe");
        assert_eq!(resolve_url("https://h/update.json", "https://o/x.exe"), "https://o/x.exe");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn check_offers_update_detects_withdrawn_and_downloads_verified() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let payload = b"MZ fake installer bytes".to_vec();
        let sha = format!("{:x}", Sha256::digest(&payload));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let manifest = format!(r#"{{"version":"v0.3.0","notes":"Новое","url":"setup.exe","sha256":"{sha}"}}"#);
        let list = LIST.to_string();
        let p2 = payload.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { break };
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body: Vec<u8> = if req.starts_with("GET /update.json") {
                    manifest.clone().into_bytes()
                } else if req.starts_with("GET /api") {
                    list.clone().into_bytes()
                } else if req.starts_with("GET /setup.exe") {
                    p2.clone()
                } else {
                    Vec::new()
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            }
        });
        let http = reqwest::Client::builder().user_agent("t").build().unwrap();
        let ch = Channel { manifest: format!("{base}/update.json"), api: Some(format!("{base}/api")) };

        let info = check(&http, &ch, "0.2.1", "", false).await;
        assert_eq!(info.available.as_ref().map(|r| r.version.as_str()), Some("0.3.0"));
        assert!(info.withdrawn.is_none());
        let skipped = check(&http, &ch, "0.2.1", "0.3.0", false).await;
        assert!(skipped.available.is_none(), "skipped version not offered in background");
        assert!(check(&http, &ch, "0.2.1", "0.3.0", true).await.available.is_some(), "manual check shows it");

        let bad = check(&http, &ch, "0.3.0", "", false).await;
        let w = bad.withdrawn.expect("running a withdrawn release");
        assert_eq!(w.reason, "ломает синхронизацию");
        assert_eq!(w.target.unwrap().version, "0.2.1");

        let dir = std::env::temp_dir().join(format!("kw-upd-{}", uuid::Uuid::new_v4().simple()));
        let mut last = 0;
        let path = download(&http, info.available.as_ref().unwrap(), &dir, |d, _| last = d).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        assert_eq!(last, payload.len() as u64);

        let mut tampered = info.available.clone().unwrap();
        tampered.sha256 = "0".repeat(64);
        assert!(download(&http, &tampered, &dir, |_, _| {}).await.is_err(), "hash mismatch rejected");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
