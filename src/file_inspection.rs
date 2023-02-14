use anyhow::Result;
use content_inspector::{inspect, ContentType};
use git2::{ObjectType, Odb, OdbLookupFlags, Oid};


use std::io;
use std::io::{Read, Write};


const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const MAX_FILE_SIZE: u64 = 5 * MB;

const EXCLUDE_PACKAGES: &[&str] = &[
    // Just a bunch of python files containing base64 encoded contents
    "pydwf",
    // Gigantic, not even needed anymore. Same package as tensorflow.
    "tensorflow-gpu",
    "tensorflow-cpu",
    // Nightly tensorflow packages account for a _lot_ of space
    "tf-nightly",
    "tf-nightly-cpu",
    "tensorflow-io-nightly",
    "tf-nightly-intel",
    "tf-nightly-cpu-aws",
    // Other misc nightly packages in the top 10
    "pyagrum-nightly",
];
//
// lazy_static! {
//     static ref EXCLUDE_REGEX: RegexSet = RegexSet::new([
//         // Huge translation files in ais-dom-frontend and home-assistant-frontend
//         "static/translations/.*\\w{32}.json$",
//
//         // Top file extensions include PKG-INFO, html and JS. We don't really want those.
//         "\\.dist-info/",
//         "LICENSE",
//         "\\.js\\.LICENSE\\.txt$",
//         "\\.pyc$",
//         "\\.js$",
//         "\\.map$",
//         "\\.po$",
//         // CSS and HTTP stuff
//         "\\.css$",
//         "\\.scss$",
//         "\\.less$",
//         // Model files (icub_models)
//         "\\.stl$",
//         "\\.dae$",
//         "\\.scz$",
//
//         // Ignore egg_info stuff, except for "entry_points.txt"
//         "\\.egg-info/(setup_requires|dependency_links|depends|zip-safe|not-zip-safe|top_level|SOURCES|requires)",
//
//         // _possibly_ skip checkpoints?
//         // "/\\.ipynb_checkpoints/"
//
//         // Specific annoyances
//         "PKG-INFO$",
//         "(^|/)\\.git(/|$)",
//         "(^|/)__pycache__(/|$)",
//     ]).unwrap();
// }
//
// // Some files are useful to include, but can be super needlessly large (think big datasets).
// // Here we can filter them out.
// const MAX_FILE_SIZES_BY_SUFFIX: &[(&str, u64)] = &[
//     (".json", MB),
//     (".geojson", MB),
//     (".csv", MB),
//     (".txt", 2 * MB),
//     (".svg", 5 * KB),
//     (".c", 2 * MB),
//     (".cpp", 2 * MB),
//     (".html", 15 * KB),
//     (".ipynb", 5 * MB),
//     // pyedflib contains large EDF files
//     (".edf", MB),
//     (".log", 3 * MB),
// ];

pub fn is_excluded_package(package_name: &str) -> bool {
    EXCLUDE_PACKAGES.contains(&package_name)
}

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
    // Pyarmor files are just big bundles of bytecode. This isn't helpful and causes
    // large repositories. They appear to always start with this token.
    if first.starts_with("__pyarmor".as_ref()) {
        return Ok(None);
    }
    // Ignore git LFS files
    if first.starts_with("version https://git-lfs".as_ref()) {
        return Ok(None);
    }
    // The code below doesn't appear to work correctly with threads.
    // let mut writer = odb.writer(size as usize, ObjectType::Blob)?;
    // writer.write_all(first)?;
    // io::copy(&mut reader, &mut writer)?;
    // Ok(Some(writer.finalize()?))
    let mut vec = Vec::with_capacity(size as usize);
    vec.extend_from_slice(first);
    io::copy(&mut reader, &mut vec)?;
    let object_hash = Oid::hash_object(ObjectType::Blob, &vec).unwrap();
    // Does the object exist already? Not thread safe, obviously.
    if odb.exists_ext(object_hash, OdbLookupFlags::NO_REFRESH) {
        return Ok(None)
    }
    Ok(Some(odb.write(ObjectType::Blob, &vec)?))
}

pub fn skip_archive_entry(name: &str, size: u64) -> bool {
    if !(1..=MAX_FILE_SIZE).contains(&size) {
        return true;
    }
    // if MAX_FILE_SIZES_BY_SUFFIX
    //     .iter()
    //     .any(|(suffix, max_size)| name.ends_with(suffix) && size > *max_size)
    // {
    //     return true;
    // }

    if !name.ends_with(".py") {
        return true
    }

    // if INCLUDE_REGEX.is_match(name.as_bytes()) {
    //     return false
    // }

    // if EXCLUDE_REGEX.is_match(name) {
    //     return true;
    // }

    false
}
