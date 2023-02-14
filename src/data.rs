use chrono::{DateTime, Utc};
use itertools::Itertools;
use jwalk::{rayon, WalkDir};
use lazy_static::lazy_static;
use rand::seq::SliceRandom;
use rand::thread_rng;
use rayon::prelude::*;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::ops::Index;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
struct Info {
    version: String,
}

#[derive(Debug, Deserialize)]
struct Url {
    url: String,
    upload_time_iso_8601: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct PackageVersion {
    info: Info,
    urls: Vec<Url>,
}

lazy_static! {
    static ref BASIC_VERSION_REGEX: Regex =
        Regex::new("(?P<major>[0-9]+)(\\.(?P<minor>[0-9]+)?)(\\.(?P<patch>[0-9]+))?").unwrap();
}

pub fn extract_urls(
    dir: PathBuf,
    output_dir: PathBuf,
    limit: Option<usize>,
    find: Option<String>,
    split: usize,
) {
    let find = find.map(|v| {
        v.split('\n')
            .flat_map(|l| l.split(' '))
            .filter(|v| !v.is_empty())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.starts_with('#'))
            .collect::<Vec<_>>()
    });

    let files_iter = WalkDir::new(dir)
        .min_depth(2)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file());
    //.sorted_by_key(|v| v.file_name.clone())
    // .collect();

    let mut files: Vec<_> = match find {
        None => files_iter.collect(),
        Some(matches) => files_iter
            .filter(|e| matches.iter().any(|m| e.file_name.to_str().unwrap() == m))
            .collect(),
    };

    let files = match limit {
        None => files,
        Some(v) => {
            files.shuffle(&mut thread_rng());
            files.into_iter().take(v).collect()
        }
    };

    files.into_par_iter().for_each(|entry| {
        let reader = BufReader::new(File::open(entry.path()).unwrap());
        let version: HashMap<String, PackageVersion> = serde_json::from_reader(reader).unwrap();

        let entry_path = entry.path();
        let package_name = entry_path.file_stem().unwrap().to_str().unwrap();

        let packages_to_download: Vec<_> = version
            .into_iter()
            .filter(|(_, v)| !v.urls.is_empty())
            .flat_map(|(_, v)| {
                let sort_key = match BASIC_VERSION_REGEX.captures(&v.info.version) {
                    None => None,
                    Some(c) => {
                        let major =
                            usize::from_str(<&str>::from(c.name("major").unwrap())).unwrap();
                        let minor = c
                            .name("minor")
                            .map(|v| usize::from_str(v.as_str()).unwrap())
                            .unwrap_or(0);
                        let patch = c
                            .name("patch")
                            .map(|v| usize::from_str(v.as_str()).unwrap())
                            .unwrap_or(0);
                        Some((major, minor, patch))
                    }
                };
                v.urls.into_iter().map(move |v2| PackageInfo {
                    version: v.info.version.clone(),
                    url: v2.url.parse().unwrap(),
                    uploaded_on: v2.upload_time_iso_8601,
                    sort_key,
                    index: 0,
                })
            })
            .sorted_by_key(|v| v.get_total_sort_key())
            .chunks(split)
            .into_iter()
            .enumerate()
            .map(|(idx, chunks)| {
                (
                    idx,
                    chunks
                        .into_iter()
                        .enumerate()
                        .map(|(idx, mut p)| {
                            p.index = idx;
                            p
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        if packages_to_download.is_empty() {
            return;
        }
        for (chunk, packages) in packages_to_download {
            let output_file_name = format!("{package_name}_{chunk}.json");
            let output = DownloadJob {
                info: JobInfo {
                    name: package_name.to_string(),
                    total: packages.len(),
                    chunk,
                },
                packages,
            };

            let output_file = File::create(output_dir.join(output_file_name)).unwrap();
            let writer = BufWriter::new(output_file);
            serde_json::to_writer(writer, &output).unwrap();
        }
    });
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct DownloadJob {
    pub info: JobInfo,
    pub packages: Vec<PackageInfo>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct JobInfo {
    pub name: String,
    pub chunk: usize,
    pub total: usize,
}

impl Display for JobInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.name, self.chunk)
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct PackageInfo {
    pub version: String,
    pub url: url::Url,
    pub uploaded_on: DateTime<Utc>,
    pub sort_key: Option<(usize, usize, usize)>,
    pub index: usize,
}

impl PackageInfo {
    pub fn get_total_sort_key(&self) -> impl Ord {
        (self.sort_key, self.version.clone(), self.uploaded_on)
    }

    pub fn package_filename(&self) -> &str {
        self.url.path_segments().unwrap().last().unwrap()
    }
}
