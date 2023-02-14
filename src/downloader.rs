use std::fs::File;
use std::io;
use std::path::PathBuf;
use crate::data::PackageInfo;
use rayon::prelude::*;
use reqwest::blocking::Client;
use tempdir::TempDir;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"), );

pub fn download_multiple(packages: Vec<PackageInfo>) -> Vec<(PackageInfo, TempDir, PathBuf)> {
    let mut downloaded_packages: Vec<_> = packages.into_par_iter().map_init(|| {
        Client::builder()
            .http2_prior_knowledge()
            .http2_adaptive_window(true)
            .user_agent(APP_USER_AGENT)
            .build()
            .unwrap()
    }, |client, info| {
        let download_dir = TempDir::new("download").unwrap();
        let download_response = client.get(info.url.clone()).send().unwrap();
        let mut download_response = download_response.error_for_status().unwrap();
        let save_path = download_dir.path().join("download");
        let mut writer = io::BufWriter::new(File::create(&save_path).unwrap());
        let mut reader = io::BufReader::new(download_response);
        io::copy(&mut reader, &mut writer).unwrap();
        (info, download_dir, save_path)
    }).collect();

    downloaded_packages.sort_by_key(|(v, _, _)| v.get_total_sort_key());
    downloaded_packages
}