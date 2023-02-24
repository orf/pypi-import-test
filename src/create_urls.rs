use chrono::{DateTime, Utc};
use itertools::Itertools;
use jwalk::{rayon, WalkDir};
use std::cmp::Ordering;

use rayon::prelude::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

use crate::utils::create_pbar;
use indicatif::ParallelProgressIterator;
use rand::seq::SliceRandom;
use rand::thread_rng;

#[derive(Debug, Deserialize, Serialize)]
struct Url {
    url: String,
    upload_time_iso_8601: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct PackageVersion {
    urls: Vec<Url>,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct DownloadJob {
    pub name: String,
    pub version: String,
    pub url: url::Url,
    pub uploaded_on: DateTime<Utc>,
}

impl PartialOrd<Self> for DownloadJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DownloadJob {
    fn cmp(&self, other: &Self) -> Ordering {
        self.uploaded_on.cmp(&other.uploaded_on)
    }
}

impl DownloadJob {
    pub fn package_filename(&self) -> &str {
        self.url.path_segments().unwrap().last().unwrap()
    }
}

const EXCLUDE_PACKAGES: &[&str] = &[
    // Just a bunch of python files containing base64 encoded contents
    "pydwf",
    // Gigantic, not even needed anymore. Same package as tensorflow.
    "tensorflow-gpu",
    "tensorflow-cpu",
    // Nightly tensorflow packages account for a _lot_ of space
    "tf-nightly",
    "tf-nightly-cpu",
    "tensorflow-io-nightly",
    "tf-nightly-intel",
    "tf-nightly-cpu-aws",
    // Other misc nightly packages in the top 10
    "pyagrum-nightly",
];

pub fn is_excluded_package(package_name: &str) -> bool {
    EXCLUDE_PACKAGES.contains(&package_name)
}

pub fn extract_urls(
    dir: PathBuf,
    output_dir: PathBuf,
    limit: Option<usize>,
    find: Option<Vec<String>>,
    split: usize,
) {
    let find = find.map(|v| {
        v.into_iter()
            .flat_map(|v| {
                v.split('\n')
                    .flat_map(|l| l.split(' '))
                    .filter(|v| !v.is_empty())
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.starts_with('#'))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });

    let files_iter = WalkDir::new(dir)
        .min_depth(2)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file());

    let mut files: Vec<_> = match find {
        None => files_iter.collect(),
        Some(matches) => files_iter
            .filter(|e| matches.iter().any(|m| e.file_name.to_str().unwrap() == m))
            .collect(),
    };

    let files = match limit {
        None => files,
        Some(_) => {
            files.shuffle(&mut thread_rng());
            files.into_iter().take(split).collect()
        }
    };

    let pbar = create_pbar(files.len() as u64, "Extracting URLs");

    let mut all_urls: Vec<_> = files
        .into_par_iter()
        .progress_with(pbar)
        .flat_map(|entry| {
            let reader = BufReader::new(File::open(entry.path()).unwrap());
            let versions: HashMap<String, PackageVersion> =
                serde_json::from_reader(reader).unwrap();
            let entry_path = entry.path();
            let package_name = entry_path.file_stem().unwrap().to_str().unwrap();
            if is_excluded_package(package_name) {
                return vec![];
            }
            versions
                .into_iter()
                .flat_map(|(version, package_info)| {
                    package_info.urls.into_iter().map(move |url| DownloadJob {
                        name: package_name.to_string(),
                        version: version.clone(),
                        url: url.url.parse().unwrap(),
                        uploaded_on: url.upload_time_iso_8601,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();
    all_urls.sort();

    let chunks: Vec<Vec<_>> = all_urls
        .into_iter()
        .chunks(split)
        .into_iter()
        .map(|v| v.collect())
        .collect();
    let chunks = match limit {
        None => chunks,
        Some(limit) => chunks.into_iter().rev().skip(1).take(limit).collect(),
    };

    for (idx, chunk) in chunks.into_iter().enumerate() {
        let output_file_name = format!("chunk_{idx}.json");
        let output_file = File::create(output_dir.join(output_file_name)).unwrap();
        let writer = BufWriter::new(output_file);
        serde_json::to_writer(writer, &chunk).unwrap();
    }
}

// pub fn extract_urls(
//     dir: PathBuf,
//     output_dir: PathBuf,
//     limit: Option<usize>,
//     find: Option<String>,
//     split: usize,
// ) {
//     let files_iter: Vec<_> = WalkDir::new(dir)
//         .min_depth(2)
//         .into_iter()
//         .flatten()
//         .filter(|e| e.file_type().is_file()).collect();
//
//     let mut counts: HashMap<(i32, u32), usize> = HashMap::new();
//
//     for path in files_iter.into_iter().progress() {
//         let reader = BufReader::new(File::open(path.path()).unwrap());
//         let versions: HashMap<String, PackageVersion> = serde_json::from_reader(reader).unwrap();
//
//         for version in versions.into_values() {
//             for url in version.urls {
//                 let month = url.upload_time_iso_8601.month();
//                 let year = url.upload_time_iso_8601.year();
//                 match counts.entry((year, month)) {
//                     Entry::Occupied(mut v) => {
//                         v.insert(v.get() + 1);
//                     }
//                     Entry::Vacant(v) => {
//                         v.insert(1);
//                     }
//                 }
//             }
//         }
//     }
//
//     let mut total = 0;
//
//     for ((year, month), value) in counts.into_iter().sorted_by_cached_key(|(k, v)| k.clone()) {
//         total += value;
//         println!("{year}-{month} {value} (cum = {total})");
//     }
// }
