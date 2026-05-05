//! File logging + panic hook.

use crate::paths;
use anyhow::Result;
use log::{error, LevelFilter};
use simplelog::{ConfigBuilder, WriteLogger};

pub fn init() -> Result<()> {
    let dir = paths::data_local_dir()?;
    std::fs::create_dir_all(&dir)?;

    let log_path = paths::log_path()?;
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > 1_000_000 {
            let _ = std::fs::remove_file(dir.join("log.3.txt"));
            let _ = std::fs::rename(dir.join("log.2.txt"), dir.join("log.3.txt"));
            let _ = std::fs::rename(dir.join("log.1.txt"), dir.join("log.2.txt"));
            let _ = std::fs::rename(&log_path, dir.join("log.1.txt"));
        }
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| match s.to_lowercase().as_str() {
            "debug" => Some(LevelFilter::Debug),
            "trace" => Some(LevelFilter::Trace),
            "warn" => Some(LevelFilter::Warn),
            "error" => Some(LevelFilter::Error),
            _ => None,
        })
        .unwrap_or(LevelFilter::Info);

    WriteLogger::init(
        level,
        ConfigBuilder::new().set_time_format_rfc3339().build(),
        file,
    )?;

    let crash_dir = dir.clone();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("{info}");
        error!("PANIC: {msg}");
        let name = format!(
            "crash-{}.txt",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let crash = crash_dir.join(name);
        let _ = std::fs::write(crash, &msg);
    }));

    Ok(())
}
