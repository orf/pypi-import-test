use crate::data::{JobInfo, PackageInfo};

use git2::{
    Buf, Index, IndexEntry, IndexTime, Mempack, ObjectType, Odb, Oid, Repository, Signature, Time,
};
use log::{info, warn};

use crate::archive::{FileContent, PackageArchive};
use reqwest::blocking::Client;
use std::io::Write;

pub fn flush_repo(
    repo: &Repository,
    mut repo_idx: Index,
    object_db: &Odb,
    mempack_backend: Mempack,
) {
    info!("Queue consumed, writing packfile");
    let mut buf = Buf::new();
    mempack_backend.dump(repo, &mut buf).unwrap();

    let mut writer = object_db.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();
    repo_idx.write().unwrap();
}

pub fn commit(
    repo: &Repository,
    repo_idx: &mut Index,
    job_info: &JobInfo,
    i: PackageInfo,
    index: Vec<TextFile>,
) -> usize {
    let filename = i.package_filename();
    let index_time = IndexTime::new(i.uploaded_on.timestamp() as i32, 0);
    let total_bytes = index.iter().map(|v| v.size).sum::<usize>();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(i.uploaded_on.timestamp(), 0),
    )
    .unwrap();

    warn!(
        "[{} {}/{}] Starting adding {} entries ({} mb)",
        job_info,
        i.index,
        job_info.total,
        index.len(),
        total_bytes / 1024 / 1024
    );
    let total = index.len();
    for text_file in index.into_iter() {
        let entry = IndexEntry {
            ctime: index_time,
            mtime: index_time,
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: text_file.size as u32,
            id: text_file.oid,
            flags: 0,
            flags_extended: 0,
            path: text_file.path,
        };
        repo_idx.add(&entry).unwrap();
    }

    let oid = repo_idx.write_tree().unwrap_or_else(|e| {
        panic!(
            "Error writing {} {}/{} {} {}: {}",
            job_info, i.index, job_info.total, i.version, i.url, e
        )
    });

    let tree = repo.find_tree(oid).unwrap();

    let parent = match &repo.head() {
        Ok(v) => Some(v.peel_to_commit().unwrap()),
        Err(_) => None,
    };
    let parent = match &parent {
        None => vec![],
        Some(p) => vec![p],
    };
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        format!("{} {} ({})", job_info.name, i.version, filename).as_str(),
        &tree,
        &parent,
    )
    .unwrap();
    warn!(
        "[{} {}/{}] Committed {} entries",
        job_info, i.index, job_info.total, total
    );

    total_bytes
}

pub struct TextFile {
    pub path: Vec<u8>,
    pub oid: Oid,
    pub size: usize,
}

const IGNORED_SUFFIXES: &[&str] = &[
    // Skip METADATA files. These can contain gigantic readme files which can bloat the repo?
    ".dist-info/METADATA",
    // Same for license files
    ".dist-info/LICENSE",
    ".dist-info/RECORD",
    ".dist-info/TOP_LEVEL",
    ".dist-info/DESCRIPTION.rst",
];

pub fn run<'a>(
    client: &mut Client,
    info: &'a JobInfo,
    item: PackageInfo,
    repo_odb: &Odb,
) -> anyhow::Result<Option<(&'a JobInfo, PackageInfo, Vec<TextFile>)>> {
    warn!("[{} {}/{}] Starting", info, item.index, info.total);
    let package_filename = item.package_filename();

    let package_extension = package_filename.rsplit('.').next().unwrap();
    // The package filename contains the package name and the version. We don't need this in the output, so just ignore it.
    // The format is `{name}-{version}-{rest}`, so we strip out `rest`
    let reduced_package_filename =
        &package_filename[(info.name.len() + 1 + item.version.len() + 1)..];

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

    let mut has_any_text_files = false;

    let mut entries = Vec::with_capacity(1024);
    warn!("[{} {}/{}] Begin iterating", info, item.index, info.total);

    for (file_name, content) in archive.all_items().flatten() {
        if let FileContent::Text(content) = content {
            if IGNORED_SUFFIXES.iter().any(|s| file_name.ends_with(s))
                || file_name.contains("/.git/")
                || file_name.ends_with("/.git")
            {
                continue;
            }
            let file_name = if file_name.starts_with(&tar_gz_first_segment) {
                &file_name[tar_gz_first_segment.len()..]
            } else {
                &*file_name
            };
            let path = format!(
                "code/{}/{}/{}/{file_name}",
                info.name, item.version, reduced_package_filename
            )
            .replace("/./", "/")
            .replace("/../", "/");

            let oid = repo_odb.write(ObjectType::Blob, &content).unwrap();
            entries.push(TextFile {
                path: path.into_bytes(),
                oid,
                size: content.len(),
            });
            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        return Ok(None);
    }

    warn!(
        "[{} {}/{}] Finished iterating",
        info, item.index, info.total
    );

    Ok(Some((info, item, entries)))
}
