#[cfg(all(windows, not(debug_assertions)))]
use std::process::Command;

#[cfg(all(windows, not(debug_assertions)))]
use anyhow::Context;

use tauri_plugin_autostart::ManagerExt;

#[cfg(all(windows, not(debug_assertions)))]
use crate::brand;

#[tauri::command]
pub fn get_start_at_login(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    crate::require_main_window(&window)?;
    app.autolaunch().is_enabled().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn set_start_at_login(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<bool, String> {
    crate::require_main_window(&window)?;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|err| err.to_string())?;
    } else {
        manager.disable().map_err(|err| err.to_string())?;
    }
    manager.is_enabled().map_err(|err| err.to_string())
}

pub fn ensure_release_desktop_shortcut() {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        if let Err(err) = create_desktop_shortcut() {
            eprintln!("Could not create Vellora desktop shortcut: {err:#}");
        }
    }
}

#[cfg(all(windows, not(debug_assertions)))]
fn create_desktop_shortcut() -> anyhow::Result<()> {
    let desktop = dirs::desktop_dir().context("Could not find Windows desktop directory")?;
    let shortcut_path = desktop.join(format!("{}.lnk", brand::APP_NAME));
    if shortcut_path.exists() {
        return Ok(());
    }

    let exe_path = std::env::current_exe().context("Could not locate Vellora executable")?;
    let working_dir = exe_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .to_path_buf();
    let script = format!(
        "$shell = New-Object -ComObject WScript.Shell; \
         $shortcut = $shell.CreateShortcut({}); \
         $shortcut.TargetPath = {}; \
         $shortcut.WorkingDirectory = {}; \
         $shortcut.IconLocation = {}; \
         $shortcut.Save()",
        ps_quote(shortcut_path.display()),
        ps_quote(exe_path.display()),
        ps_quote(working_dir.display()),
        ps_quote(format!("{},0", exe_path.display())),
    );

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
        .context("Could not launch PowerShell to create desktop shortcut")?;

    if !status.success() {
        anyhow::bail!("PowerShell shortcut creation exited with {status}");
    }
    Ok(())
}

#[cfg(all(windows, not(debug_assertions)))]
fn ps_quote(value: impl std::fmt::Display) -> String {
    let value = value.to_string().replace('\'', "''");
    format!("'{value}'")
}
