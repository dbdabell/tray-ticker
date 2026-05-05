//! JSON config under `%APPDATA%\\tray-ticker\\config.json`.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default = "default_symbol")]
    pub symbol: String,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default = "default_last_range")]
    pub last_range: String,
}

fn default_last_range() -> String {
    "1D".into()
}

fn default_symbol() -> String {
    "AAPL".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            symbol: "AAPL".into(),
            autostart: false,
            last_range: "1D".into(),
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("", "", "tray-ticker").context("no valid ProjectDirs")?;
    Ok(dirs.config_dir().join("config.json"))
}

pub fn load() -> Config {
    let Ok(path) = config_path() else {
        return Config::default();
    };
    if !path.exists() {
        return Config::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(cfg)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let cfg = Config {
            symbol: "MSFT".into(),
            autostart: true,
            last_range: "1W".into(),
        };
        let s = serde_json::to_string(&cfg).unwrap();
        let read: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(read, cfg);
    }

    #[test]
    fn empty_json_uses_defaults() {
        let cfg: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.symbol, "AAPL");
        assert!(!cfg.autostart);
        assert_eq!(cfg.last_range, "1D");
    }
}
