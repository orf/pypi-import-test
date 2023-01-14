mod archive;

use crate::archive::{FileContent, PackageArchive};

use clap::Parser;
use fs_extra::dir::CopyOptions;
use git2::{
    Cred, Direction, IndexEntry, IndexTime, ObjectType, Oid, PushOptions, RemoteCallbacks,
    Repository, Signature,
};
use std::io;
use std::path::{Path, PathBuf};
use tempdir::TempDir;
use url::Url;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long, short)]
    repo: PathBuf,

    #[command(subcommand)]
    run_type: RunType,
}

#[derive(clap::Subcommand)]
enum RunType {
    FromArgs {
        #[arg()]
        name: String,

        #[arg()]
        version: String,

        #[arg()]
        url: Url,
    },
    FromStdin {},
}

#[derive(serde::Deserialize, Debug)]
struct JsonInput {
    name: String,
    version: String,
    url: Url,
}

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();

    match args.run_type {
        RunType::FromArgs { name, version, url } => run(args.repo, name, version, url)?,
        RunType::FromStdin {} => {
            let stdin = io::stdin();
            let input: JsonInput = serde_json::from_reader(stdin).unwrap();
            run(args.repo, input.name, input.version, input.url)?
        }
    }
    Ok(())
}

fn run(repo: PathBuf, name: String, version: String, url: Url) -> anyhow::Result<()> {
    let package_filename = url.path_segments().unwrap().last().unwrap();

    let package_extension = package_filename.rsplit('.').next().unwrap();

    // Copy our git directory to a temporary directory
    let tmp_dir = TempDir::new("git-import")?;
    let options = CopyOptions::new();
    fs_extra::dir::copy(repo, &tmp_dir, &options).unwrap();
    // Create the repo and grab the main branch, and the index
    let repo = Repository::open(tmp_dir.path().join("pypi-code-import"))?;
    let main = repo.revparse_single("main")?;
    let mut index = repo.index()?;

    let download_response = reqwest::blocking::get(url.clone())?;
    let mut archive = match PackageArchive::new(package_extension, download_response) {
        None => {
            panic!("Unknown extension {package_extension}")
        }
        Some(v) => v,
    };

    let mut has_any_text_files = false;

    for (name, content) in archive.all_items().flatten() {
        // Skip METADATA files. These can contain gigantic readme files which can bloat the repo?
        if name.ends_with(".dist-info/METADATA") {
            continue;
        }
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
                path: format!("package/{name}").into(),
            };
            index.add_frombuffer(&entry, &content).unwrap();

            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        println!("No files!");
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
            format!("{name} {version}").as_str(),
            &tree,
            &[parent_commit],
        )
        .unwrap();
    let x = repo.find_commit(commit_oid)?;

    let new_branch_name = format!("{}/{}/{}", &name, &version, package_filename);

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
    callbacks.push_update_reference(|r, error| {
        if let Some(e) = error {
            panic!("Error pushing {r}: {e}")
        } else {
            println!("Pushed {r}");
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
