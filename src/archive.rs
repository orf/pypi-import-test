use std::io;
use std::io::{BufReader, Read};

use bzip2::read::BzDecoder;
use content_inspector::{inspect, ContentType};
use flate2::read::GzDecoder;
use reqwest::blocking::Response;
use tar::{Archive, Entries};
use zip::read::read_zipfile_from_stream;

const MAX_FILE_SIZE: u64 = 1024 * 1024 * 50;

pub enum PackageArchive {
    Zip(BufReader<Response>),
    TarGz(Archive<GzDecoder<Response>>),
    TarBz(Archive<BzDecoder<Response>>),
}

impl PackageArchive {
    pub fn new(extension: &str, reader: Response) -> Option<Self> {
        match extension {
            "egg" | "zip" | "whl" | "exe" => Some(PackageArchive::Zip(BufReader::with_capacity(
                1024 * 1024 * 12,
                reader,
            ))),
            "gz" => {
                let tar = GzDecoder::new(reader);
                let archive = Archive::new(tar);
                Some(PackageArchive::TarGz(archive))
            }
            "bz2" => {
                let tar = BzDecoder::new(reader);
                let archive = Archive::new(tar);
                Some(PackageArchive::TarBz(archive))
            }
            _ => None,
        }
    }

    #[inline(always)]
    pub fn all_items(&mut self) -> PackageEnumIterator {
        match self {
            PackageArchive::Zip(z) => PackageEnumIterator::Zip(z),
            PackageArchive::TarGz(t) => PackageEnumIterator::TarGz(t.entries().unwrap()),
            PackageArchive::TarBz(t) => PackageEnumIterator::TarBz(t.entries().unwrap()),
        }
    }
}

pub enum PackageEnumIterator<'a> {
    Zip(&'a mut BufReader<Response>),
    TarGz(Entries<'a, GzDecoder<Response>>),
    TarBz(Entries<'a, BzDecoder<Response>>),
}

impl<'a> Iterator for PackageEnumIterator<'a> {
    type Item = anyhow::Result<(String, FileContent)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            PackageEnumIterator::Zip(v) => loop {
                return match read_zipfile_from_stream(v) {
                    Ok(z) => match z {
                        None => None,
                        Some(z) => {
                            if !z.is_file() {
                                continue;
                            }
                            if z.size() > MAX_FILE_SIZE {
                                continue;
                            }
                            let name = z.name().to_string();
                            let content = inspect_content(z);
                            Some(Ok((name, content)))
                        }
                    },
                    Err(_) => None,
                };
            },
            PackageEnumIterator::TarGz(t) => match t.flatten().find(|v| v.size() != 0) {
                None => None,
                Some(v) => {
                    if v.size() > MAX_FILE_SIZE {
                        return None;
                    }
                    let name = v.path().unwrap().to_str().unwrap().to_string();
                    let content = inspect_content(v);
                    Some(Ok((name, content)))
                }
            },
            PackageEnumIterator::TarBz(t) => match t.flatten().find(|v| v.size() != 0) {
                None => None,
                Some(v) => {
                    if v.size() > MAX_FILE_SIZE {
                        return None;
                    }
                    let name = v.path().unwrap().to_str().unwrap().to_string();
                    let content = inspect_content(v);
                    Some(Ok((name, content)))
                }
            },
        }
    }
}

#[derive(Debug)]
pub enum FileContent {
    Binary,
    Text(Vec<u8>),
}

#[inline(always)]
fn inspect_content<R: Read>(mut item: R) -> FileContent {
    let mut first = [0; 1024];
    let n = item.read(&mut first[..]).unwrap();
    let content_type = inspect(&first[..n]);
    if content_type == ContentType::BINARY {
        return FileContent::Binary;
    }
    let mut read_vec = first[..n].to_vec();
    io::copy(&mut item, &mut read_vec).unwrap();
    FileContent::Text(read_vec)
}
