use anyhow::Result;
use content_inspector::{inspect, ContentType};
use git2::{ObjectType, Odb, Oid};
use log::debug;
use std::io;
use std::io::{Read, Write};

const MAX_FILE_SIZE: u64 = 1024 * 1024 * 15;
const MIN_FILE_SIZE: u64 = 1;

// Top file extensions include PKG-INFO, html and JS. We don't really want those.
const EXCLUDE_SUFFIXES: &[&str] = &[
    "/PKG-INFO",
    ".dist-info/METADATA",
    ".dist-info/RECORD",
    ".dist-info/LICENSE",
    ".dist-info/top_level.txt",
    "LICENSE",
    ".pyc",
    ".js",
    ".map",
    ".html",
    ".po",
    ".css",
    ".c",
    ".cpp",
];

pub fn write_archive_entry_to_odb<R: Read>(
    size: u64,
    mut reader: &mut R,
    odb: &Odb,
) -> Result<Option<Oid>> {
    let mut first = [0; 1024];
    let n = reader.read(&mut first[..])?;
    let first = &first[..n];
    let content_type = inspect(first);
    if content_type == ContentType::BINARY {
        return Ok(None);
    }
    let mut writer = odb.writer(size as usize, ObjectType::Blob)?;
    writer.write_all(first)?;
    io::copy(&mut reader, &mut writer)?;
    Ok(Some(writer.finalize()?))
}

pub fn skip_archive_entry(name: &str, size: u64) -> bool {
    if !(MIN_FILE_SIZE..=MAX_FILE_SIZE).contains(&size) {
        debug!("Path {name} has size {size}, skipping");
        return true;
    }
    for suffix in EXCLUDE_SUFFIXES {
        if name.ends_with(suffix) {
            debug!("Name {} ends with suffix {}", name, suffix);
            return true;
        }
    }
    // if EXCLUDE_SUFFIXES.iter().any(|s| name.ends_with(s)) {
    //     return true;
    // }
    if name.contains("/.git/") || name.contains("/__pycache__/") {
        debug!("Path {name} contains /.git/ or /__pycache__/");
        return true;
    }
    debug!("Not skipping {name}");
    false
}
