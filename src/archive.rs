use anyhow::anyhow;

use std::io::Read;

use bzip2::read::BzDecoder;

use crate::file_inspection::{skip_archive_entry, write_archive_entry_to_odb};
use flate2::read::GzDecoder;
use git2::{Odb, Oid};

use tar::{Archive, Entries};
use zip::read::read_zipfile_from_stream;

pub type PackageReader = Box<dyn Read>;

pub enum PackageArchive {
    Zip(PackageReader),
    TarGz(Box<Archive<GzDecoder<PackageReader>>>),
    TarBz(Box<Archive<BzDecoder<PackageReader>>>),
}

impl PackageArchive {
    pub fn new(extension: &str, reader: PackageReader) -> Option<Self> {
        match extension {
            "egg" | "zip" | "whl" | "exe" => Some(PackageArchive::Zip(reader)),
            "gz" => {
                let tar = GzDecoder::new(reader);
                let archive = Archive::new(tar);
                Some(PackageArchive::TarGz(Box::new(archive)))
            }
            "bz2" => {
                let tar = BzDecoder::new(reader);
                let archive = Archive::new(tar);
                Some(PackageArchive::TarBz(Box::new(archive)))
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
    Zip(&'a mut PackageReader, &'a Odb<'a>),
    TarGz(Entries<'a, GzDecoder<PackageReader>>, &'a Odb<'a>),
    TarBz(Entries<'a, BzDecoder<PackageReader>>, &'a Odb<'a>),
}

impl<'a> Iterator for PackageEnumIterator<'a> {
    type Item = anyhow::Result<(String, Oid)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            PackageEnumIterator::Zip(v, odb) => loop {
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
                            let write_result = write_archive_entry_to_odb(&name, size, &mut z, odb);
                            match write_result {
                                Ok(v) => match v {
                                    None => continue,
                                    Some(oid) => Some(Ok((name, oid))),
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
) -> Option<anyhow::Result<(String, Oid)>> {
    let iterator = items.into_iter().flatten().flat_map(|v| {
        let path = v
            .path()?
            .to_str()
            .ok_or(anyhow!("Error converting path to string"))?
            .to_string();
        let size = v.size();
        Ok::<_, anyhow::Error>((path, size, v))
    });
    find_item(iterator, odb)
}

fn find_item(
    mut items: impl Iterator<Item = (String, u64, impl Read)>,
    odb: &Odb,
) -> Option<anyhow::Result<(String, Oid)>> {
    while let Some((path, size, mut reader)) = items.find(|(_, size, _)| *size != 0) {
        match handle_tar_gz(path, size, &mut reader, odb) {
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
    path: String,
    size: u64,
    mut reader: impl Read,
    odb: &Odb,
) -> anyhow::Result<Option<(String, Oid)>> {
    if skip_archive_entry(&path, size) {
        return Ok(None);
    }
    if let Some(oid) = write_archive_entry_to_odb(&path, size, &mut reader, odb)? {
        return Ok(Some((path, oid)));
    }
    Ok(None)
}
