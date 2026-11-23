use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Own OAuth "Desktop app" client; empty = the client built into the app.
    pub google_client_id: String,
    pub google_client_secret: String,
    pub reminders_enabled: bool,
    /// Reminders ring in a loop and stay on screen until the user acts on them.
    pub alarm: bool,
    /// Windows looping sound: "Alarm", "Alarm2".."Alarm10", "Call".."Call10".
    pub alarm_sound: String,
    pub notify_changes: bool,
    pub poll_focused_secs: u64,
    pub poll_background_secs: u64,
    /// UI preferences, stored here so they survive reinstalling the WebView cache.
    pub view: String,
    pub sidebar: bool,
    pub day_start_hour: u32,
    /// "system" | "dark" | "light"
    pub theme: String,
    /// "system" (Windows display language) | "ru" | "en"
    pub language: String,
    /// Optional push relay (see `relay/`). Empty = polling only.
    pub relay_url: String,
    /// Secret channel key shared with the relay; generated on first use.
    pub relay_key: String,
    /// Check GitHub releases at startup and every 6 hours.
    pub check_updates: bool,
    /// Version the user chose to skip; background checks do not offer it.
    pub skipped_version: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            google_client_id: String::new(),
            google_client_secret: String::new(),
            reminders_enabled: true,
            alarm: true,
            alarm_sound: "Alarm".into(),
            notify_changes: true,
            poll_focused_secs: 30,
            poll_background_secs: 180,
            view: "week".into(),
            sidebar: true,
            day_start_hour: 8,
            theme: "system".into(),
            language: "system".into(),
            relay_url: String::new(),
            relay_key: String::new(),
            check_updates: true,
            skipped_version: String::new(),
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
        crate::i18n::apply(&cfg.language);
        cfg
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let tmp = dir.join("config.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, Self::path(dir))?;
        Ok(())
    }

    pub fn has_client(&self) -> bool {
        !self.client().0.is_empty()
    }

    /// The OAuth client to use: the user's own if set, else the one baked in at build time.
    pub fn client(&self) -> (String, String) {
        let id = self.google_client_id.trim();
        if !id.is_empty() {
            return (id.to_string(), self.google_client_secret.trim().to_string());
        }
        (
            option_env!("KW_BUILTIN_CLIENT_ID").unwrap_or_default().to_string(),
            option_env!("KW_BUILTIN_CLIENT_SECRET").unwrap_or_default().to_string(),
        )
    }

    /// This build ships its own OAuth client: users only need to sign in.
    pub fn builtin_client() -> bool {
        option_env!("KW_BUILTIN_CLIENT_ID").is_some_and(|s| !s.is_empty())
    }
}
