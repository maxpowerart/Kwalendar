//! Bakes the app's Google OAuth "Desktop app" client into the binary, so users only sign in.
//! Source: env KW_GOOGLE_CLIENT_ID / KW_GOOGLE_CLIENT_SECRET (CI secrets), else the untracked
//! `oauth.local.json` at the repository root: {"client_id": "...", "client_secret": "..."}.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=KW_GOOGLE_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=KW_GOOGLE_CLIENT_SECRET");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../oauth.local.json");
    println!("cargo:rerun-if-changed={}", file.display());

    let mut id = std::env::var("KW_GOOGLE_CLIENT_ID").ok().filter(|s| !s.is_empty());
    let mut secret = std::env::var("KW_GOOGLE_CLIENT_SECRET").ok();
    if id.is_none() {
        if let Ok(text) = std::fs::read_to_string(&file) {
            id = json_field(&text, "client_id");
            secret = secret.or_else(|| json_field(&text, "client_secret"));
        }
    }
    if let Some(id) = id {
        println!("cargo:rustc-env=KW_BUILTIN_CLIENT_ID={id}");
        println!("cargo:rustc-env=KW_BUILTIN_CLIENT_SECRET={}", secret.unwrap_or_default());
    }
}

/// Tiny reader for a flat JSON object of strings (build scripts avoid extra dependencies).
fn json_field(text: &str, name: &str) -> Option<String> {
    let key = format!("\"{name}\"");
    let rest = &text[text.find(&key)? + key.len()..];
    let rest = &rest[rest.find(':')? + 1..];
    let start = rest.find('"')? + 1;
    let end = start + rest[start..].find('"')?;
    Some(rest[start..end].trim().to_string()).filter(|s| !s.is_empty())
}
