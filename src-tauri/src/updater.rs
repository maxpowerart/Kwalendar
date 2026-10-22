//! Update check, version picker and install-with-restart (GitHub releases, see `kw_core::update`).
//!
//! Install: the NSIS installer is downloaded and verified, then a hidden PowerShell waits for
//! this process to exit, runs the installer silently and starts the new version. Going back to
//! an older version first runs the installed uninstaller (user data in %APPDATA% stays).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use kw_core::update::{self, Channel, Listed, Release, UpdateInfo};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;

const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct Updates {
    info: Mutex<UpdateInfo>,
    busy: Mutex<bool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    done: u64,
    total: u64,
}

fn current(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

async fn run_check(app: &AppHandle, manual: bool) -> UpdateInfo {
    let Some(s) = app.try_state::<AppState>() else { return UpdateInfo::default() };
    let cfg = s.core.config();
    let info = update::check(&s.core.http, &Channel::from_env(), &current(app), &cfg.skipped_version, manual).await;
    if let Some(u) = app.try_state::<Updates>() {
        *u.info.lock().unwrap() = info.clone();
    }
    let _ = app.emit("kw:update", &info);
    info
}

/// Startup + every 6 h, unless switched off in settings (or `KW_NO_UPDATE_CHECK`).
pub async fn background(app: AppHandle) {
    tokio::time::sleep(Duration::from_secs(10)).await;
    loop {
        let enabled = app
            .try_state::<AppState>()
            .map(|s| s.core.config().check_updates)
            .unwrap_or(false)
            && std::env::var_os("KW_NO_UPDATE_CHECK").is_none();
        if enabled {
            let info = run_check(&app, false).await;
            if let Some(r) = &info.available {
                log::info!("update available: {}", r.version);
            }
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

#[tauri::command]
pub fn update_info(app: AppHandle, u: State<Updates>) -> UpdateInfo {
    let mut i = u.info.lock().unwrap().clone();
    i.current = current(&app);
    i
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateInfo {
    run_check(&app, true).await
}

#[tauri::command]
pub async fn update_list(s: State<'_, AppState>) -> Result<Vec<Listed>, String> {
    update::list_releases(&s.core.http, &Channel::from_env()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_manifest(s: State<'_, AppState>, url: String) -> Result<Release, String> {
    update::fetch_manifest(&s.core.http, &url).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_skip(s: State<AppState>, app: AppHandle, version: String) -> Result<(), String> {
    let mut cfg = s.core.config();
    cfg.skipped_version = version;
    s.core.set_config(cfg).map_err(|e| e.to_string())?;
    if let Some(u) = app.try_state::<Updates>() {
        u.info.lock().unwrap().available = None;
    }
    Ok(())
}

/// Folder of an NSIS installation (`uninstall.exe` next to the running exe).
fn install_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    dir.join("uninstall.exe").exists().then_some(dir)
}

fn ps_quote(p: &std::path::Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "''"))
}

#[tauri::command]
pub async fn update_install(app: AppHandle, release: Release) -> Result<(), String> {
    let dir = install_dir().ok_or_else(|| {
        "Это не установленная копия (сборка для разработки) — установите Kwalendar установщиком, чтобы обновляться из приложения"
            .to_string()
    })?;
    let u = app.state::<Updates>();
    {
        let mut busy = u.busy.lock().unwrap();
        if *busy {
            return Err("Обновление уже скачивается".into());
        }
        *busy = true;
    }
    let s = app.state::<AppState>();
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    let app2 = app.clone();
    let res = update::download(&s.core.http, &release, &cache, move |done, total| {
        let _ = app2.emit("kw:update-progress", Progress { done, total });
    })
    .await;
    *u.busy.lock().unwrap() = false;
    let setup = res.map_err(|e| e.to_string())?;

    let downgrade = update::is_newer(&release.version, &current(&app));
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let log = cache.join("update.log");
    let mut script = format!(
        "Start-Transcript -Path {} -Force | Out-Null; $ErrorActionPreference='Continue'; Wait-Process -Id {} -Timeout 60 -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500; ",
        ps_quote(&log),
        std::process::id()
    );
    if downgrade {
        script.push_str(&format!(
            "$u = Start-Process -FilePath {} -ArgumentList '/S','_?={}' -PassThru; $u.WaitForExit(); \"uninstall exit: $($u.ExitCode)\"; Start-Sleep -Seconds 1; ",
            ps_quote(&dir.join("uninstall.exe")),
            dir.to_string_lossy().replace('\'', "''")
        ));
    }
    script.push_str(&format!(
        "$p = Start-Process -FilePath {} -ArgumentList '/S' -PassThru; $p.WaitForExit(); \"installer exit: $($p.ExitCode)\"; Start-Process -FilePath {}; Stop-Transcript | Out-Null",
        ps_quote(&setup),
        ps_quote(&exe)
    ));
    let spawn = |flags: u32| {
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &script]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(flags);
        }
        #[cfg(not(windows))]
        let _ = flags;
        cmd.spawn()
    };
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    // Leave the parent's job (if any) so the updater survives our exit; not every job allows it.
    spawn(CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| spawn(CREATE_NO_WINDOW))
        .map_err(|e| format!("не удалось запустить установщик: {e}"))?;
    log::info!("installing {} ({}), restarting", release.version, if downgrade { "rollback" } else { "update" });
    app.exit(0);
    Ok(())
}
