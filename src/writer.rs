use crate::extract_urls::{DownloadJob, JobInfo, PackageInfo};

use git2::{
    Buf, FileMode, Index, IndexEntry, IndexTime, Mempack, ObjectType, Odb, Repository, Signature,
    Time, Tree, TreeWalkMode,
};
use log::{error, info, warn};

use crate::archive::PackageArchive;

use git2::build::TreeUpdateBuilder;

use serde::{Deserialize, Serialize};

use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use crate::{downloader, file_inspection};

pub enum PackageResult {
    Complete,
    Empty,
    Excluded,
}

pub fn flush_repo(
    info: &JobInfo,
    repo: &Repository,
    mut repo_idx: Index,
    object_db: &Odb,
    mempack_backend: Mempack,
) {
    match repo.head() {
        Ok(h) => {
            let commit = h.peel_to_commit().unwrap();
            repo.branch(&format!("{}-{}", info.name, info.chunk), &commit, true)
                .unwrap();
        }
        Err(e) => {
            panic!("Could not get repo head? {e}");
        }
    }

    info!("Queue consumed, writing packfile");
    let mut buf = Buf::new();
    mempack_backend.dump(repo, &mut buf).unwrap();

    let mut writer = object_db.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();
    repo_idx.write().unwrap();
}

fn merge_tree<'a>(tree: &Tree, repo: &'a Repository, base_tree: &Tree) -> Tree<'a> {
    let mut update = TreeUpdateBuilder::new();
    tree.walk(TreeWalkMode::PostOrder, |x, y| {
        // code/adb3/1.1.0/tar.gz/ -> 4 splits.
        if let (4, Some(ObjectType::Tree)) = (x.split('/').count(), y.kind()) {
            update.upsert(
                format!("{}{}", x, y.name().unwrap()),
                y.id(),
                FileMode::Tree,
            );
            return 1; // Don't dive deeper
        }
        0
    })
    .unwrap();
    let new_tree_oid = update.create_updated(repo, base_tree).unwrap();
    repo.find_tree(new_tree_oid).unwrap()
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CommitMessage<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub file: &'a str,
    pub path: String,
}

pub fn commit(
    repo: &Repository,
    job_info: &JobInfo,
    i: &PackageInfo,
    index: &mut Index,
    code_path: String,
) {
    let filename = i.package_filename();
    // let index_time = IndexTime::new(i.uploaded_on.timestamp() as i32, 0);
    // let total_bytes = index.iter().map(|v| v.size).sum::<usize>();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(i.uploaded_on.timestamp(), 0),
    )
    .unwrap();

    let oid = index.write_tree_to(repo).unwrap_or_else(|e| {
        panic!(
            "Error writing {} {}/{} {} {} (idx len {}): {}",
            job_info,
            i.index,
            job_info.total,
            i.version,
            i.url,
            index.len(),
            e,
        )
    });

    let tree = repo.find_tree(oid).unwrap();

    let parent = match &repo.head() {
        Ok(v) => {
            let commit = v.peel_to_commit().unwrap();
            // let commit_tree = commit.tree().unwrap();
            // tree = merge_tree(&tree, repo, &commit_tree);
            Some(commit)
        }
        Err(_) => None,
    };

    let parent = match &parent {
        None => vec![],
        Some(p) => vec![p],
    };
    let commit_message = serde_json::to_string(&CommitMessage {
        name: &job_info.name,
        version: &i.version,
        file: filename,
        path: code_path,
    })
    .unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &commit_message,
        &tree,
        &parent,
    )
    .unwrap();
    warn!(
        "[{} {}/{}] Committed {} entries",
        job_info,
        i.index,
        job_info.total,
        index.len()
    );
}

pub fn package_name_to_path<'a>(
    name: &'a String,
    version: &'a str,
    package_filename: &'a str,
) -> (&'a str, &'a str, &'a str) {
    // The package filename contains the package name and the version. We don't need this in the output, so just ignore it.
    // The format is `{name}-{version}-{rest}`, so we strip out `rest`
    // Some packages, like `free-valorant-points-redeem-code-v-3693.zip`, don't fit this convention.
    // In this case just return the extension.
    // We also need to normalize the underscores in the name.
    let name_version = format!("{}_{}", name, version).replace('-', "_");
    let reduced_filename = match package_filename
        .replace('-', "_")
        .starts_with(&name_version)
    {
        true => &package_filename[(name_version.len() + 1)..],
        false => package_filename.rsplit('.').next().unwrap(),
    };
    (name, version, reduced_filename)
}

pub fn run(
    archive_path: PathBuf,
    // client: &mut Client,
    info: &JobInfo,
    item: &PackageInfo,
    repo_odb: &Odb,
    index: &mut Index,
) -> anyhow::Result<Option<String>> {
    let package_filename = item.package_filename();
    let package_extension = package_filename.rsplit('.').next().unwrap();

    let code_prefix = package_name_to_path(&info.name, &item.version, package_filename);
    let code_prefix = format!("code/{}/{}/{}", code_prefix.0, code_prefix.1, code_prefix.2);

    let tar_gz_first_segment = format!("{}-{}/", info.name, item.version);

    let mut archive =
        match PackageArchive::new(package_extension, File::open(archive_path).unwrap()) {
            None => {
                return Ok(None);
            }
            Some(v) => v,
        };

    let mut file_count: usize = 0;

    // let mut index = Index::new().unwrap();
    let index_time = IndexTime::new(item.uploaded_on.timestamp() as i32, 0);

    for v in archive.all_items(repo_odb) {
        let (file_name, size, oid) = match v {
            Ok(v) => v,
            Err(e) => {
                error!(
                    "[{} {}/{}] Error iterating: {}",
                    info, item.index, info.total, e
                );
                continue;
            }
        };
        let file_name = if file_name.starts_with(&tar_gz_first_segment) {
            &file_name[tar_gz_first_segment.len()..]
        } else {
            &*file_name
        };

        // Some paths are weird. A release in backports.ssl_match_hostname contains
        // files with double slashes: `src/backports/ssl_match_hostname//backports.ssl_match_hostname-3.4.0.1.tar.gz.asc`
        // This might be an issue with my code somewhere, but everything else seems to be fine.
        let path = format!("{}/{file_name}", code_prefix)
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
            file_size: size as u32,
            id: oid,
            flags: 0,
            flags_extended: 0,
            path: path.into_bytes(),
        };
        index.add(&entry).unwrap();
        file_count += 1;
    }

    if file_count == 0 {
        warn!("[{} {}/{}] No files added", info, item.index, info.total);
        return Ok(None);
    }
    Ok(Some(code_prefix))
}

pub fn run_multiple(repo_path: &PathBuf, job: DownloadJob) -> anyhow::Result<PackageResult> {
    if file_inspection::is_excluded_package(&job.info.name) {
        return Ok(PackageResult::Excluded);
    }

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

    // let (sender, recv) = unbounded();

    let odb = repo.odb().unwrap();
    let mempack_backend = odb.add_new_mempack_backend(3).unwrap();
    let mut repo_idx = repo.index().unwrap();

    let mut has_any_files = false;

    let files = downloader::download_multiple(job.packages);

    for (info, temp_dir, download_path) in files {
        if let Some(code_path) = run(download_path, &job.info, &info, &odb, &mut repo_idx)
            .unwrap_or_else(|_| panic!("Error with job {}", job.info))
        {
            has_any_files = true;
            commit(&repo, &job.info, &info, &mut repo_idx, code_path)
        }
        drop(temp_dir) // delete
    }

    if has_any_files {
        flush_repo(&job.info, &repo, repo_idx, &odb, mempack_backend);
    }

    if has_any_files {
        Ok(PackageResult::Complete)
    } else {
        Ok(PackageResult::Empty)
    }
}
