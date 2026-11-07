//! UI language for texts produced outside the WebView: errors, toasts, the tray menu.
//!
//! The language is a process-wide setting (`Config::language`), applied whenever the config is
//! loaded. `"system"` follows the Windows display language: Russian if it is Russian, otherwise
//! English.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(true);

/// The Windows display language as "ru" or "en".
pub fn system_lang() -> &'static str {
    if system_is_russian() {
        "ru"
    } else {
        "en"
    }
}

#[cfg(windows)]
fn system_is_russian() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    const LANG_RUSSIAN: u16 = 0x19;
    // SAFETY: takes no arguments and only reads the user's language setting.
    let id = unsafe { GetUserDefaultUILanguage() };
    id & 0x3ff == LANG_RUSSIAN
}

#[cfg(not(windows))]
fn system_is_russian() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.starts_with("ru"))
}

/// `pref`: "ru" | "en" | anything else = follow the system.
pub fn apply(pref: &str) {
    let en = match pref {
        "ru" => false,
        "en" => true,
        _ => system_lang() == "en",
    };
    ENGLISH.store(en, Ordering::Relaxed);
}

pub fn is_en() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Picks the Russian or English text; both arms take the same `format!` arguments.
///
/// `tr!("Через {n} мин", "In {n} min")`
#[macro_export]
macro_rules! tr {
    ($ru:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_en() { format!($en $(, $arg)*) } else { format!($ru $(, $arg)*) }
    };
}
