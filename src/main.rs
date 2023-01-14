mod archive;

use crate::archive::{FileContent, PackageArchive};
use clap::Parser;
use fs_extra::dir::CopyOptions;
use git2::{
    Cred, Direction, IndexEntry, IndexTime, ObjectType, Oid, PushOptions, RemoteCallbacks,
    Repository, Signature,
};
use std::path::{Path, PathBuf};
use tempdir::TempDir;
use url::Url;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg()]
    repo: PathBuf,

    #[arg()]
    package_name: String,

    #[arg()]
    package_version: String,

    #[arg()]
    package_url: Url,
}

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();

    let package_filename = args.package_url.path_segments().unwrap().last().unwrap();

    let package_extension = package_filename.rsplit('.').next().unwrap();

    // Copy our git directory to a temporary directory
    let tmp_dir = TempDir::new("git-import")?;
    let options = CopyOptions::new();
    fs_extra::dir::copy(&args.repo, &tmp_dir, &options).unwrap();
    // Create the repo and grab the main branch, and the index
    let repo = Repository::open(tmp_dir.path().join("pypi-code-import"))?;
    let main = repo.revparse_single("main")?;
    let mut index = repo.index()?;

    let download_response = reqwest::blocking::get(args.package_url.clone())?;
    let mut archive = PackageArchive::new(package_extension, download_response);

    let mut has_any_text_files = false;

    for (name, content) in archive.all_items().flatten() {
        if let FileContent::Text(content) = content {
            let hash = Oid::hash_object(ObjectType::Blob, &content).unwrap();
            let entry = IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: hash,
                flags: 0,
                flags_extended: 0,
                path: format!("package/{}", name).into(),
            };
            index.add_frombuffer(&entry, &content).unwrap();

            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        return Ok(());
    }

    let oid = index.write_tree().unwrap();
    let signature = Signature::now("Tom Forbes", "tom@tomforb.es").unwrap();
    let parent_commit = main.as_commit().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    let commit_oid = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "test commit",
            &tree,
            &[parent_commit],
        )
        .unwrap();
    let x = repo.find_commit(commit_oid)?;

    let new_branch_name = format!(
        "{}/{}/{}",
        &args.package_name, &args.package_version, package_filename
    );

    repo.branch(&new_branch_name, &x, true).unwrap();

    fn create_callbacks<'a>() -> RemoteCallbacks<'a> {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.credentials(|_str, _str_opt, _cred_type| {
            Cred::ssh_key(
                "git",
                Some(Path::new(
                    &shellexpand::tilde("~/.ssh/id_rsa.pub").to_string(),
                )),
                Path::new(&shellexpand::tilde("~/.ssh/id_rsa").to_string()),
                None,
            )
        });
        callbacks
    }

    let mut remote = repo.find_remote("origin").unwrap();

    remote
        .connect_auth(Direction::Push, Some(create_callbacks()), None)
        .unwrap();
    repo.remote_add_push(
        "origin",
        &format!("refs/heads/{new_branch_name}:refs/heads/{new_branch_name}"),
    )
    .unwrap();

    let mut push_options = PushOptions::default();
    let mut callbacks = create_callbacks();
    callbacks.push_update_reference(|_r, error| {
        if let Some(e) = error {
            panic!("Error pushing! {e}")
        }
        Ok(())
    });

    push_options.remote_callbacks(callbacks);
    remote
        .push(
            &[format!(
                "+refs/heads/{new_branch_name}:refs/heads/{new_branch_name}"
            )],
            Some(&mut push_options),
        )
        .unwrap();
    Ok(())
}

//
// fn main() -> anyhow::Result<()> {
//     let args: Cli = Cli::parse();
//     let mut all_entries: Vec<DirEntry> = fs::read_dir(args.input)?.flatten().collect();
//     all_entries.shuffle(&mut thread_rng());
//     println!("Total: {}", all_entries.len());
//     fs::create_dir_all(&args.done_dir).unwrap();
//     fs::create_dir_all(&args.output).unwrap();
//     let all_entries = &all_entries[0..args.limit];
//     let size = all_entries.len();
//     let pbar = ProgressBar::new(size as u64);
//
//     let style =
//         ProgressStyle::with_template("{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} ({eta})")
//             .unwrap();
//     pbar.set_style(style);
//     all_entries
//         .into_par_iter()
//         .progress_with(pbar)
//         .for_each(|entry| {
//             let path = entry.path();
//             let output_path = args.output.join(path.file_name().unwrap());
//             let rename_path = path.clone();
//             if let Some(ext) = path.extension().and_then(OsStr::to_str) {
//                 match ext {
//                     "gz" => {
//                         let output_txt_file = File::create(output_path).unwrap();
//                         let mut writer = BufWriter::new(output_txt_file);
//                         for mut entry in archive
//                             .entries()
//                             .unwrap()
//                             .flatten()
//                             .filter(|v| v.size() != 0)
//                         {
//                             let mut first = [0; 1024];
//                             let n = entry.read(&mut first[..]).unwrap();
//                             let content_type = inspect(&first[..n]);
//
//                             if content_type == ContentType::BINARY {
//                                 continue;
//                             }
//                             writer.write_all(&first[..n]).unwrap();
//                             io::copy(&mut entry, &mut writer).unwrap();
//                         }
//                     }
//                     "egg" | "zip" | "whl" => {
//                         let zip_file =
//                             BufReader::new(File::open(path).unwrap());
//                         if let Ok(mut archive) = zip::ZipArchive::new(zip_file) {
//                             let output_txt_file = File::create(output_path).unwrap();
//                             let mut writer = BufWriter::new(output_txt_file);
//                             (0..archive.len()).for_each(|i| {
//                                 let mut entry = archive.by_index(i).unwrap();
//                                 if !entry.is_file() || entry.size() == 0 {
//                                     return;
//                                 }
//                                 let mut first = [0; 1024];
//                                 let n = entry.read(&mut first[..]).unwrap();
//                                 let content_type = inspect(&first[..n]);
//                                 if content_type == ContentType::BINARY {
//                                     return;
//                                 }
//                                 writer.write_all(&first[..n]).unwrap();
//                                 io::copy(&mut entry, &mut writer).unwrap();
//                             })
//                         }
//                     }
//                     _ => panic!("Unhandled extension {ext}"),
//                 }
//             };
//
//             let fname = rename_path.file_name().unwrap();
//             let new = args.done_dir.join(fname);
//             fs::rename(&rename_path, new).unwrap();
//         });
//     Ok(())
// }
