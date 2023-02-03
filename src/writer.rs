use crate::JsonInput;
use crossbeam::channel::Receiver;
use git2::{Buf, Index, IndexEntry, IndexTime, ObjectType, Odb, Repository, Signature, Time};
use log::{info, warn};

use std::io::Write;

pub fn consume_queue(repo: &Repository, recv: Receiver<(JsonInput, Vec<TextFile>, String)>) {
    let mut repo_idx = repo.index().unwrap();

    let object_db = repo.odb().unwrap();
    let mempack_backend = object_db.add_new_mempack_backend(3).unwrap();

    for (i, index, filename) in recv {
        commit(repo, &mut repo_idx, &object_db, i, index, filename);
    }

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
    odb: &Odb,
    i: JsonInput,
    index: Vec<TextFile>,
    filename: String,
) -> usize {
    let index_time = IndexTime::new(i.uploaded_on.timestamp() as i32, 0);

    let total_bytes = index.iter().map(|v| v.contents.len()).sum::<usize>();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(i.uploaded_on.timestamp(), 0),
    )
    .unwrap();

    warn!("Starting adding {} entries", index.len());
    info!("Total size: {}kb", total_bytes / 1024);
    let total = index.len();
    for text_file in index.into_iter() {
        let oid = odb.write(ObjectType::Blob, &text_file.contents).unwrap();
        let entry = IndexEntry {
            ctime: index_time,
            mtime: index_time,
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: text_file.contents.len() as u32,
            id: oid,
            flags: 0,
            flags_extended: 0,
            path: text_file.path.into_bytes(),
        };
        repo_idx.add(&entry).unwrap();
    }

    info!("Added {} entries, writing tree", total);
    let oid = repo_idx
        .write_tree()
        .unwrap_or_else(|e| panic!("Error writing {} {} {}: {}", i.name, i.version, i.url, e));

    info!("Written tree, fetching info from repo");
    let tree = repo.find_tree(oid).unwrap();

    let parent = match &repo.head() {
        Ok(v) => Some(v.peel_to_commit().unwrap()),
        Err(_) => None,
    };
    let parent = match &parent {
        None => vec![],
        Some(p) => vec![p],
    };
    info!("Committing info");
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        format!("{} {} ({})", i.name, i.version, filename).as_str(),
        &tree,
        &parent,
    )
    .unwrap();
    warn!("Committed {} entries", total);

    total_bytes
}

pub struct TextFile {
    pub path: String,
    pub contents: Vec<u8>,
}

// impl TextFile {
//     pub fn add(self, odb: &) {}
// }
