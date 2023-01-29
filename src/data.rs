use crate::JsonInput;
use itertools::Itertools;
use jwalk::{rayon, WalkDir};
use rand::seq::SliceRandom;
use rand::thread_rng;
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Info {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct Url {
    url: String,
}

#[derive(Debug, Deserialize)]
struct PackageVersion {
    info: Info,
    urls: Vec<Url>,
}

pub fn extract_urls(dir: PathBuf, output_dir: PathBuf, limit: Option<usize>, find: Option<String>) {
    let files_iter = WalkDir::new(dir)
        .min_depth(2)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file());
    //.sorted_by_key(|v| v.file_name.clone())
    // .collect();

    let mut files: Vec<_> = match find {
        None => files_iter.collect(),
        Some(f) => files_iter
            .filter(|e| e.file_name.to_str().unwrap().starts_with(&f))
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

        let sorted_to_download = version
            .into_iter()
            .flat_map(|(_, v)| {
                v.urls
                    .into_iter()
                    .map(move |v2| (v.info.name.clone(), v.info.version.clone(), v2.url))
            })
            .sorted_by_key(|v| v.1.clone());
        let packages_to_download: Vec<_> = sorted_to_download
            .into_iter()
            .map(|(name, version, url)| JsonInput {
                name,
                version,
                url: url.parse().unwrap(),
            })
            .collect();

        if packages_to_download.is_empty() {
            return;
        }

        let output_file = File::create(output_dir.join(entry.file_name)).unwrap();
        let writer = BufWriter::new(output_file);

        serde_json::to_writer(writer, &packages_to_download).unwrap();
    });
}
