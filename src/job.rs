use crate::archive::PackageArchive;
use crate::create_urls::DownloadJob;
use crate::downloader::download_multiple;
use git2::{
    Buf, Commit, FileMode, Index, IndexEntry, IndexTime, Mempack, Odb, Repository, Signature, Time,
};
use itertools::Itertools;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use git2::build::TreeUpdateBuilder;
use rayon::prelude::*;

#[cfg(not(feature = "no_progress"))]
use {
    crate::utils::create_pbar,
    indicatif::{ParallelProgressIterator, ProgressIterator},
};

#[derive(Debug, Deserialize, Serialize)]
pub struct CommitMessage<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub file: &'a str,
    pub path: String,
}

fn log_timer(message: &str, path: &str, previous_instant: Option<Instant>) -> Option<Instant> {
    match previous_instant {
        None => warn!("[{}] {message}", path),
        Some(p) => warn!("[{}] {message} ({}s elapsed)", path, p.elapsed().as_secs()),
    }
    Some(Instant::now())
}

pub fn run_multiple(repo_path: &PathBuf, jobs: Vec<DownloadJob>) -> anyhow::Result<()> {
    git2::opts::strict_object_creation(false);
    git2::opts::strict_hash_verification(false);

    let repo_file_name = repo_path.file_name().unwrap().to_str().unwrap();

    let repo = match Repository::open(repo_path) {
        Ok(v) => v,
        Err(_) => {
            let _ = fs::create_dir(repo_path);
            let repo = Repository::init(repo_path).unwrap();
            let mut index = repo.index().unwrap();
            index.set_version(4).unwrap();
            repo
        }
    };
    let odb = repo.odb().unwrap();
    let mempack_backend = odb.add_new_mempack_backend(3).unwrap();

    let start_timer = log_timer("Downloading", repo_file_name, None);

    let downloaded = download_multiple(jobs)?;

    #[cfg(not(feature = "no_progress"))]
    let pbar = {
        let total = downloaded.len();
        create_pbar(total as u64, "Extracting")
    };

    let timer = log_timer("Extracting", repo_file_name, start_timer);

    let mut download_results: Vec<_> = {
        let iterator = downloaded
            .into_par_iter()
            .filter_map(|(job, temp_dir, download_path)| {
                extract(&job, &download_path, &odb)
                    .unwrap()
                    .map(|(path, total_files, index)| (job, temp_dir, path, total_files, index))
            });
        #[cfg(not(feature = "no_progress"))]
        {
            iterator.progress_with(pbar)
        }
        #[cfg(feature = "no_progress")]
        {
            iterator
        }
    }
    .collect();

    download_results.sort_by(|k1, k2| k1.0.cmp(&k2.0));

    let timer = log_timer("Committing", repo_file_name, timer);

    #[cfg(not(feature = "no_progress"))]
    let pbar = create_pbar(download_results.len() as u64, "Committing");

    let mut parent_commit = repo.head().unwrap().peel_to_commit().unwrap();
    let download_iter = {
        let iterator = download_results.into_iter();
        #[cfg(not(feature = "no_progress"))]
        {
            iterator.progress_with(pbar)
        }
        #[cfg(feature = "no_progress")]
        {
            iterator
        }
    };
    for (job, temp_dir, path, _total_files, index) in download_iter {
        parent_commit = commit(&repo, index, &job, path, parent_commit);
        let _ = fs::remove_dir_all(temp_dir.path());
        drop(temp_dir);
    }

    log_timer("Flushing", repo_file_name, timer);

    let repo_index = repo.index().unwrap();
    flush_repo(&repo, repo_index, &odb, mempack_backend);

    log_timer("Done", repo_file_name, start_timer);

    Ok(())
}

pub fn extract(
    job: &DownloadJob,
    archive_path: &PathBuf,
    odb: &Odb,
) -> anyhow::Result<Option<(String, usize, Index)>> {
    let package_filename = job.package_filename();
    let package_extension = package_filename.rsplit('.').next().unwrap();
    let mut archive =
        match PackageArchive::new(package_extension, File::open(archive_path).unwrap()) {
            None => {
                return Ok(None);
            }
            Some(v) => v,
        };

    let prefix = format!("packages/{package_filename}/");
    let index_time = IndexTime::new(job.uploaded_on.timestamp() as i32, 0);
    let mut file_count = 0;

    let all_items: Vec<_> = archive
        .all_items(odb)
        .flat_map(|v| match v {
            Ok(v) => Some(v),
            Err(e) => {
                error!("Error with package {}: {e}", job.url);
                None
            }
        })
        .collect();

    // Some packages have hidden "duplicate" packages. For example there is `fs.googledrivefs` and `fs-googledrivefs`.
    // These are distinct *packages*, but `fs-googledrivefs` has releases that are also under `fs.googledrivefs`.
    // I've verified that there are currently no two files with the same name but different hashes, which is a relief,
    // but the fact two packages have the same name causes issues with the "strip first component" check.
    // Instead of checking for a specific string, we can instead check if there is a shared common prefix with
    // all files. If there is only one shared common prefix then we strip it.
    let first_segment_to_skip = if all_items.len() > 1 {
        let first_segments: Vec<_> = all_items
            .iter()
            .flat_map(|(path, _, _)| path.split('/').next())
            .sorted()
            .unique()
            .take(2)
            .collect();
        match &first_segments[..] {
            &[prefix] => Some(prefix),
            _ => None,
        }
    } else {
        None
    };

    let mut index = Index::new()?;

    for (file_name, size, oid) in &all_items {
        let file_name = match first_segment_to_skip {
            None => file_name,
            Some(to_strip) => {
                if file_name.starts_with(to_strip) {
                    &file_name[to_strip.len() + 1..]
                } else {
                    file_name
                }
            }
        };

        // Some paths are weird. A release in backports.ssl_match_hostname contains
        // files with double slashes: `src/backports/ssl_match_hostname//backports.ssl_match_hostname-3.4.0.1.tar.gz.asc`
        // This might be an issue with my code somewhere, but everything else seems to be fine.
        let path = format!("{}/{file_name}", prefix)
            .replace("/./", "/")
            .replace("/../", "/")
            .replace("//", "/");

        let entry = IndexEntry {
            ctime: index_time,
            mtime: index_time,
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: *size as u32,
            id: *oid,
            flags: 0,
            flags_extended: 0,
            path: path.into_bytes(),
        };
        index.add(&entry)?;
        file_count += 1;
    }

    if file_count == 0 {
        Ok(None)
    } else {
        Ok(Some((prefix, file_count, index)))
    }
}

pub fn commit<'a>(
    repo: &'a Repository,
    mut index: Index,
    info: &DownloadJob,
    code_path: String,
    parent: Commit,
) -> Commit<'a> {
    let filename = info.package_filename();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(info.uploaded_on.timestamp(), 0),
    )
    .unwrap();
    let tree_to_merge_oid = index.write_tree_to(repo).unwrap();
    let tree_to_merge = repo.find_tree(tree_to_merge_oid).unwrap();
    let parent_tree = parent.tree().unwrap();

    let tree_path = Path::new(&code_path);
    let inner_tree_path = tree_to_merge.get_path(tree_path).unwrap();

    let mut update = TreeUpdateBuilder::new();
    update.upsert(tree_path, inner_tree_path.id(), FileMode::Tree);
    let updated_tree_oid = update.create_updated(repo, &parent_tree).unwrap();
    let new_tree = repo.find_tree(updated_tree_oid).unwrap();

    let commit_message = serde_json::to_string(&CommitMessage {
        name: &info.name,
        version: &info.version,
        file: filename,
        path: code_path,
    })
    .unwrap();
    let oid = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            &commit_message,
            &new_tree,
            &[&parent],
        )
        .unwrap();
    repo.find_commit(oid).unwrap()
}

pub fn flush_repo(
    repo: &Repository,
    mut repo_idx: Index,
    object_db: &Odb,
    mempack_backend: Mempack,
) {
    let mut buf = Buf::new();
    mempack_backend.dump(repo, &mut buf).unwrap();

    let mut writer = object_db.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();
    repo_idx.write().unwrap();
}
