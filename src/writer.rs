use crate::data::{JobInfo, PackageInfo};

use git2::{
    Buf, FileMode, Index, IndexEntry, IndexTime, Mempack, ObjectType, Odb, Repository, Signature,
    Time, Tree, TreeWalkMode,
};
use log::{error, info, warn};

use crate::archive::PackageArchive;

use git2::build::TreeUpdateBuilder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use std::io::Write;

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
    i: PackageInfo,
    mut index: Index,
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

    let total = index.len();

    let oid = index.write_tree_to(repo).unwrap_or_else(|e| {
        panic!(
            "Error writing {} {}/{} {} {} (idx len {}): {}",
            job_info, i.index, job_info.total, i.version, i.url, total, e,
        )
    });

    let mut tree = repo.find_tree(oid).unwrap();

    let parent = match &repo.head() {
        Ok(v) => {
            let commit = v.peel_to_commit().unwrap();
            let commit_tree = commit.tree().unwrap();
            tree = merge_tree(&tree, repo, &commit_tree);
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
        job_info, i.index, job_info.total, total
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
    let name_version = format!("{}-{}", name, version);
    // To-Do: Make this line work - we need to normalize underscores. Bruh.
    // let reduced_filename = match package_filename.replace('_',  "-").starts_with(&name_version) {
    let reduced_filename = match package_filename.starts_with(&name_version) {
        true => &package_filename[(name_version.len() + 1)..],
        false => package_filename.rsplit('.').next().unwrap(),
    };
    (name, version, reduced_filename)
}

pub fn run<'a>(
    client: &mut Client,
    info: &'a JobInfo,
    item: PackageInfo,
    repo_odb: &Odb,
) -> anyhow::Result<Option<(&'a JobInfo, PackageInfo, Index, String)>> {
    // warn!("[{} {}/{}] Starting", info, item.index, info.total);
    let package_filename = item.package_filename();
    let package_extension = package_filename.rsplit('.').next().unwrap();

    let code_prefix = package_name_to_path(&info.name, &item.version, package_filename);
    let code_prefix = format!("code/{}/{}/{}", code_prefix.0, code_prefix.1, code_prefix.2);

    // .tar.gz files unwrap all contents to paths like `Django-1.10rc1/...`. This isn't great,
    // so we detect this and strip the prefix.
    let tar_gz_first_segment = format!("{}-{}/", info.name, item.version);

    let download_response = client.get(item.url.clone()).send()?;
    let mut archive = match PackageArchive::new(package_extension, download_response) {
        None => {
            return Ok(None);
        }
        Some(v) => v,
    };

    let mut file_count: usize = 0;

    let mut index = Index::new().unwrap();
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

    // warn!(
    //     "[{} {}/{}] Finished iterating: {} files",
    //     info, item.index, info.total, file_count
    // );

    Ok(Some((info, item, index, code_prefix)))
}
