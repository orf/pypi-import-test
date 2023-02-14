use std::fs::File;
use anyhow::anyhow;
use std::io::{BufReader, Read};

use bzip2::read::BzDecoder;

use crate::file_inspection::{skip_archive_entry, write_archive_entry_to_odb};
use flate2::read::GzDecoder;
use git2::{Odb, Oid};
use reqwest::blocking::Response;
use tar::{Archive, Entries, Entry};
use zip::read::read_zipfile_from_stream;

pub enum PackageArchive {
    Zip(BufReader<File>),
    TarGz(Archive<GzDecoder<File>>),
    TarBz(Archive<BzDecoder<File>>),
}

impl PackageArchive {
    pub fn new(extension: &str, file: File) -> Option<Self> {
        match extension {
            "egg" | "zip" | "whl" | "exe" => Some(PackageArchive::Zip(BufReader::with_capacity(
                1024 * 1024 * 12,
                file,
            ))),
            "gz" => {
                let tar = GzDecoder::new(file);
                let archive = Archive::new(tar);
                Some(PackageArchive::TarGz(archive))
            }
            "bz2" => {
                let tar = BzDecoder::new(file);
                let archive = Archive::new(tar);
                Some(PackageArchive::TarBz(archive))
            }
            _ => None,
        }
    }

    pub fn all_items<'a>(&'a mut self, odb: &'a Odb<'a>) -> PackageEnumIterator<'a> {
        match self {
            PackageArchive::Zip(z) => PackageEnumIterator::Zip(z, odb),
            PackageArchive::TarGz(t) => PackageEnumIterator::TarGz(t.entries().unwrap(), odb),
            PackageArchive::TarBz(t) => PackageEnumIterator::TarBz(t.entries().unwrap(), odb),
        }
    }
}

pub enum PackageEnumIterator<'a> {
    Zip(&'a mut BufReader<File>, &'a Odb<'a>),
    TarGz(Entries<'a, GzDecoder<File>>, &'a Odb<'a>),
    TarBz(Entries<'a, BzDecoder<File>>, &'a Odb<'a>),
}

impl<'a> Iterator for PackageEnumIterator<'a> {
    type Item = anyhow::Result<(String, u64, Oid)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            PackageEnumIterator::Zip(v, odb) => loop {
                // To-do: this now doesn't need to read from a stream.
                return match read_zipfile_from_stream(v) {
                    Ok(z) => match z {
                        None => None,
                        Some(mut z) => {
                            if !z.is_file() {
                                continue;
                            }
                            if skip_archive_entry(z.name(), z.size()) {
                                continue;
                            }
                            let name = z.name().to_string();
                            let size = z.size();
                            match write_archive_entry_to_odb(size, &mut z, odb) {
                                Ok(v) => match v {
                                    None => continue,
                                    Some(oid) => Some(Ok((name, size, oid))),
                                },
                                Err(e) => Some(Err(e)),
                            }
                        }
                    },
                    Err(_) => None,
                };
            },
            PackageEnumIterator::TarGz(t, odb) => find_tar_item(t, odb),
            PackageEnumIterator::TarBz(t, odb) => find_tar_item(t, odb),
        }
    }
}

fn find_tar_item(
    items: &mut Entries<impl Read>,
    odb: &Odb,
) -> Option<anyhow::Result<(String, u64, Oid)>> {
    while let Some(mut z) = items.flatten().find(|v| v.size() != 0) {
        match handle_tar_gz(&mut z, odb) {
            Ok(v) => match v {
                None => continue,
                Some(v) => return Some(Ok(v)),
            },
            Err(e) => return Some(Err(e)),
        }
    }
    None
}

fn handle_tar_gz(
    mut z: &mut Entry<impl Read>,
    odb: &Odb,
) -> anyhow::Result<Option<(String, u64, Oid)>> {
    let path = z
        .path()?
        .to_str()
        .ok_or(anyhow!("Error converting path to string"))?
        .to_string();
    let size = z.size();
    if skip_archive_entry(&path, size) {
        return Ok(None);
    }
    if let Some(oid) = write_archive_entry_to_odb(size, &mut z, odb)? {
        return Ok(Some((path, size, oid)));
    }
    Ok(None)
}
