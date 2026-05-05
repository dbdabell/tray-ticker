//! Canonical paths under ProjectDirs.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

fn dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "tray-ticker").context("invalid ProjectDirs")
}

pub fn data_local_dir() -> Result<PathBuf> {
    Ok(dirs()?.data_local_dir().to_path_buf())
}

pub fn show_request_path() -> Result<PathBuf> {
    Ok(data_local_dir()?.join("show.request"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(data_local_dir()?.join("log.txt"))
}
