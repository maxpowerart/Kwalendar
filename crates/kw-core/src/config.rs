use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// OAuth "Desktop app" client from Google Cloud Console.
    pub google_client_id: String,
    pub google_client_secret: String,
    pub reminders_enabled: bool,
    pub notify_changes: bool,
    pub poll_focused_secs: u64,
    pub poll_background_secs: u64,
    /// UI preferences, stored here so they survive reinstalling the WebView cache.
    pub view: String,
    pub sidebar: bool,
    pub day_start_hour: u32,
    /// Optional push relay (see `relay/`). Empty = polling only.
    pub relay_url: String,
    /// Secret channel key shared with the relay; generated on first use.
    pub relay_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            google_client_id: option_env!("KW_GOOGLE_CLIENT_ID").unwrap_or_default().to_string(),
            google_client_secret: option_env!("KW_GOOGLE_CLIENT_SECRET").unwrap_or_default().to_string(),
            reminders_enabled: true,
            notify_changes: true,
            poll_focused_secs: 30,
            poll_background_secs: 180,
            view: "week".into(),
            sidebar: true,
            day_start_hour: 8,
            relay_url: String::new(),
            relay_key: String::new(),
        }
    }
}

impl Config {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("config.json")
    }

    pub fn load(dir: &Path) -> Self {
        let mut cfg: Config = std::fs::read_to_string(Self::path(dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Env overrides for development.
        if let Ok(id) = std::env::var("KW_GOOGLE_CLIENT_ID") {
            cfg.google_client_id = id;
        }
        if let Ok(s) = std::env::var("KW_GOOGLE_CLIENT_SECRET") {
            cfg.google_client_secret = s;
        }
        cfg.poll_focused_secs = cfg.poll_focused_secs.clamp(10, 600);
        cfg.poll_background_secs = cfg.poll_background_secs.clamp(30, 3600);
        cfg
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let tmp = dir.join("config.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, Self::path(dir))?;
        Ok(())
    }

    pub fn has_client(&self) -> bool {
        !self.google_client_id.trim().is_empty()
    }
}
