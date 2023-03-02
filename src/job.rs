use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::archive::{PackageArchive, PackageReader};
use crate::create_urls::DownloadJob;

use anyhow::Context;
use git2::{Buf, Commit, FileMode, Index, Mempack, Odb, Oid, Repository, Signature, Time};
use itertools::Itertools;
use log::{error, warn};
use serde::{Deserialize, Serialize};

use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::time::Duration;
use std::{fs, io};

use git2::build::TreeUpdateBuilder;
use rayon::prelude::*;
use ureq::Agent;
use url::Url;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CommitMessage {
    pub name: String,
    pub version: String,
    pub file: String,
    pub path: PathBuf,
}

pub static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

pub fn run_multiple(repo_path: &PathBuf, jobs: Vec<DownloadJob>) -> anyhow::Result<()> {
    git2::opts::strict_object_creation(false);
    git2::opts::strict_hash_verification(false);

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

    let baseline_tree_oid = repo.treebuilder(None)?.write()?;

    // I get quite a few DNS errors when using MacOS. I'm not sure why, but we could just avoid any
    // DNS overhead by re-using existing addresses? Fastly uses static anycast IPs, so why do we
    // need to re-resolve them ever?
    let dns_result: Result<Vec<_>, _> = "files.pythonhosted.org:443"
        .to_socket_addrs()
        .map(Iterator::collect);
    let dns_result = dns_result?;

    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_read(Duration::from_secs(30))
        .user_agent(APP_USER_AGENT)
        .resolver(move |addr: &str| match addr {
            "files.pythonhosted.org:443" => Ok(dns_result.clone()),
            _ => panic!("Unexpected address {addr}"),
        })
        .build();

    fn download_with_retry(agent: &mut Agent, url: &Url) -> anyhow::Result<Option<Vec<u8>>> {
        for _i in 0..5 {
            let response = match agent.get(url.as_str()).call() {
                Ok(response) => Ok::<_, anyhow::Error>(response),
                Err(ureq::Error::Status(404, _)) => return Ok(None),
                Err(ureq::Error::Status(416, _)) => return Ok(None),
                Err(e) => Err(e.into()),
            }
            .with_context(|| format!("Error fetching URL {}", url))?;

            let mut data = match response.header("Content-Length") {
                None => {
                    vec![]
                }
                Some(v) => Vec::with_capacity(v.parse()?),
            };

            match response.into_reader().read_to_end(&mut data) {
                Ok(_) => return Ok(Some(data)),
                Err(e) => {
                    warn!("{url} failed: {e}");
                    continue;
                }
            }
        }
        warn!("Skipping {url} due to 5 errors");
        Ok(None)
    }

    let extracted_packages = jobs
        .into_par_iter()
        .map_init(
            || {
                let agent = agent.clone();
                let output_repo = Repository::open(repo_path).unwrap();
                output_repo.set_odb(&odb).unwrap();
                (agent, output_repo)
            },
            |(agent, repo), job| {
                let data = match download_with_retry(agent, &job.url)? {
                    None => return Ok((job, None)),
                    Some(d) => d,
                };
                let reader = io::Cursor::new(data);

                let item =
                    extract(&job, &odb, reader, repo, &baseline_tree_oid).with_context(|| {
                        format!(
                            "Error processing {} / {} / {}",
                            job.name,
                            job.version,
                            job.package_filename()
                        )
                    })?;
                Ok::<_, anyhow::Error>((job, item))
            },
        )
        .collect::<Result<Vec<_>, _>>()?;

    for (job, result) in extracted_packages {
        if let Some((path, tree_oid)) = result {
            commit(&repo, &job, path, tree_oid);
        }
    }

    let repo_index = repo.index().unwrap();
    flush_repo(&repo, repo_index, &odb, mempack_backend);
    Ok(())
}

pub fn extract(
    job: &DownloadJob,
    odb: &Odb,
    reader: PackageReader,
    repo: &mut Repository,
    baseline_tree_oid: &Oid,
) -> anyhow::Result<Option<(String, Oid)>> {
    let package_filename = job.package_filename();
    let package_extension = package_filename.rsplit('.').next().unwrap();
    let mut archive = match PackageArchive::new(package_extension, reader) {
        None => {
            return Ok(None);
        }
        Some(v) => v,
    };

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
        // Some releases (btf_extractor-1.6.0-cp39-cp39-win_amd64.whl) have multiple zip entries for the same files.
        // This is... really annoying. I'm paranoid though - what if someone uses this to "hide" some code?
        // We could (should?) detect this by also hashing the OID, and renaming the file if there
        // is a collision?
        .unique_by(|(name, _)| {
            let mut s = DefaultHasher::new();
            name.hash(&mut s);
            // oid.hash(&mut s);
            s.finish()
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
            .flat_map(|(path, _)| path.split('/').next())
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

    let mut tree_builder = TreeUpdateBuilder::new();

    for (original_file_name, oid) in &all_items {
        let file_name = match first_segment_to_skip {
            None => original_file_name,
            Some(to_strip) => {
                if original_file_name.starts_with(to_strip) {
                    &original_file_name[to_strip.len() + 1..]
                } else {
                    original_file_name
                }
            }
        };

        // Some paths are weird. A release in backports.ssl_match_hostname contains
        // files with double slashes: `src/backports/ssl_match_hostname//backports.ssl_match_hostname-3.4.0.1.tar.gz.asc`
        // This might be an issue with my code somewhere, but everything else seems to be fine.
        let path = format!("/{file_name}")
            .replace("/./", "/")
            .replace("/../", "/")
            .replace("//", "/")
            // .git/ isn't a valid path. Some packages do have python files within .git!
            .replace("/.git/", "/dot-git/");
        let path_without_slash = path.trim_start_matches('/');

        tree_builder.upsert(path_without_slash, *oid, FileMode::Blob);
        file_count += 1;
    }

    let baseline_tree = repo.find_tree(*baseline_tree_oid)?;
    let tree_oid = tree_builder.create_updated(repo, &baseline_tree)?;

    let package_prefix = format!("packages/{}/{package_filename}", job.name);

    if file_count == 0 {
        Ok(None)
    } else {
        Ok(Some((package_prefix, tree_oid)))
    }
}

pub fn commit<'a>(
    repo: &'a Repository,
    info: &DownloadJob,
    code_path: String,
    tree_oid: Oid,
) -> Commit<'a> {
    let filename = info.package_filename();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(info.uploaded_on.timestamp(), 0),
    )
    .unwrap();
    let commit_message = serde_json::to_string(&CommitMessage {
        name: info.name.clone(),
        version: info.version.clone(),
        file: filename.to_string(),
        path: code_path.into(),
    })
    .unwrap();
    let tree = repo.find_tree(tree_oid).unwrap();
    let oid = repo
        .commit(None, &signature, &signature, &commit_message, &tree, &[])
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
