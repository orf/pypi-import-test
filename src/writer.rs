use crate::JsonInput;
use crossbeam::channel::Receiver;
use git2::{
    Buf, Index, IndexEntry, IndexTime, Mempack, ObjectType, Odb, Repository,
    Signature, Time,
};
use log::info;
use std::io::{Write};


fn write_packfile(repo: &Repository, object_db: &Odb, mempack_backend: &Mempack) {
    let mut buf = Buf::new();
    mempack_backend.dump(repo, &mut buf).unwrap();

    let mut writer = object_db.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();
}

pub fn consume_queue(
    repo: &Repository,
    repo_idx: &mut Index,
    recv: Receiver<(JsonInput, Vec<TextFile>, String)>,
    object_db: &Odb,
    mempack_backend: Mempack,
) {
    let mut total_bytes = 0;
    let _builder = repo.packbuilder().unwrap();
    // let x = PackBuilder::name()
    for (i, index, filename) in recv {
        total_bytes += commit(repo, repo_idx, object_db, i, index, filename);
        if total_bytes > 1024 * 1024 * 250 {
            write_packfile(repo, object_db, &mempack_backend);
            info!("Packfile written, resetting and unlocking");
            mempack_backend.reset().unwrap();
            object_db
                .foreach(|v| {
                    println!("Oids after reset: {v}");
                    true
                })
                .unwrap();
            total_bytes = 0;
        }
    }

    info!("Queue consumed, writing packfile");
    write_packfile(repo, object_db, &mempack_backend);
    info!("Writing index");
    repo_idx.write().unwrap();
}

pub fn commit(
    repo: &Repository,
    repo_idx: &mut Index,
    odb: &Odb,
    i: JsonInput,
    index: Vec<TextFile>,
    filename: String,
    // builder: PackBuilder,
) -> usize {
    let index_time = IndexTime::new(i.uploaded_on.timestamp() as i32, 0);
    // let oid = odb.write(ObjectType::Blob, &content).unwrap();

    let total_bytes = index.iter().map(|v| v.contents.len()).sum::<usize>();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(i.uploaded_on.timestamp(), 0),
    )
    .unwrap();

    info!("Starting adding {} entries", index.len());
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
    info!("Committed!");

    total_bytes
}

pub struct TextFile {
    pub path: String,
    pub contents: Vec<u8>,
}

// impl TextFile {
//     pub fn add(self, odb: &) {}
// }
