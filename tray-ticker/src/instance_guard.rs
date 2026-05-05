//! Named mutex via `single-instance`; secondary instances touch `show.request` and exit.

use crate::paths;
use anyhow::Result;
use std::fs;

pub fn acquire_or_signal_show() -> Result<Option<single_instance::SingleInstance>> {
    let instance = single_instance::SingleInstance::new("tray-ticker_singleton")
        .map_err(|_| anyhow::anyhow!("failed to create single-instance mutex"))?;
    if instance.is_single() {
        return Ok(Some(instance));
    }
    if let Ok(p) = paths::show_request_path() {
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&p, b"1");
    }
    Ok(None)
}
