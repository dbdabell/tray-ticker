//! `HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\\TrayTicker`.

use anyhow::{Context, Result};
use std::path::Path;
use winreg::enums::*;
use winreg::RegKey;

const VALUE_NAME: &str = "TrayTicker";

pub fn is_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") else {
        return false;
    };
    run.get_value::<String, _>(VALUE_NAME)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

pub fn set_enabled(on: bool, exe: &Path) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if on {
        let (run, _) = hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")?;
        let s = exe.to_string_lossy().to_string();
        run.set_value(VALUE_NAME, &s).context("set Run value")?;
    } else if let Ok(run) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
        let _ = run.delete_value(VALUE_NAME);
    }
    Ok(())
}
