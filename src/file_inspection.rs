use anyhow::Result;
use content_inspector::{inspect, ContentType};
use git2::{ObjectType, Odb, Oid};
use log::debug;
use std::io;
use std::io::Read;

const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const MAX_FILE_SIZE: u64 = 15 * MB;

// Top file extensions include PKG-INFO, html and JS. We don't really want those.
const EXCLUDE_SUFFIXES: &[&str] = &[
    ".dist-info/METADATA",
    ".dist-info/RECORD",
    ".dist-info/LICENSE",
    ".dist-info/top_level.txt",
    "LICENSE",
    ".pyc",
    ".js",
    ".map",
    ".po",
    // CSS and HTTP stuff
    ".css",
    ".scss",
    ".less",
    // Model files (icub_models)
    ".stl",
    ".dae",
    // Ignore all .git files
    ".git",
];

// Some files are useful to include, but can be super needlessly large (think big datasets).
// Here we can filter them out.
const MAX_FILE_SIZES_BY_SUFFIX: &[(&str, u64)] = &[
    (".json", MB),
    (".geojson", MB),
    (".csv", MB),
    (".txt", 2 * MB),
    (".svg", 5 * KB),
    (".c", 2 * MB),
    (".cpp", 2 * MB),
    (".html", 15 * KB),
    (".ipynb", 7 * MB),
    // pyedflib contains large EDF files
    (".edf", MB),
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
    // The code below doesn't appear to work correctly.
    // let mut writer = odb.writer(size as usize, ObjectType::Blob)?;
    // writer.write_all(first)?;
    // io::copy(&mut reader, &mut writer)?;
    // Ok(Some(writer.finalize()?))
    let mut vec = Vec::with_capacity(size as usize);
    vec.extend_from_slice(first);
    io::copy(&mut reader, &mut vec)?;
    Ok(Some(odb.write(ObjectType::Blob, &vec)?))
}

pub fn skip_archive_entry(name: &str, size: u64) -> bool {
    if !(1..=MAX_FILE_SIZE).contains(&size) {
        debug!("Path {name} has size {size}, skipping");
        return true;
    }
    if name == "PKG-INFO"
        || name.starts_with(".git/")
        || name.starts_with("__pycache__/")
        || name.contains("/.git/")
        || name.contains("/__pycache__/")
    {
        return true;
    }
    if EXCLUDE_SUFFIXES.iter().any(|v| name.ends_with(v)) {
        return true;
    }
    if MAX_FILE_SIZES_BY_SUFFIX
        .iter()
        .any(|(suffix, max_size)| name.ends_with(suffix) && size > *max_size)
    {
        return true;
    }
    debug!("Not skipping {name}");
    false
}
