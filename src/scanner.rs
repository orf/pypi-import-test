use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct ScanJob {
    file_size: usize,
    package_name: String,
    package_version: String,
    creation_date: DateTime<Utc>,
    file_path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScanResult(serde_json::Value);

pub fn scan(_repo: &PathBuf, _cmd: String) {}
