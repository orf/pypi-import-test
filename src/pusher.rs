use git2::Repository;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
// use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize)]
pub struct RepoStatistics {
    // tag_name: String,
    unique_blobs: usize,
    total_size: usize,
    commits: usize,
    path: PathBuf,
    // last_commit: String,
    // push_order: Vec<String>,
}

pub fn get_repo_statistics(repo: PathBuf) -> RepoStatistics {
    // let mut tag_name = repo.file_name().unwrap().to_str().unwrap().rsplit_once('.').unwrap().0.to_string();
    // if tag_name.ends_with("_0") {
    //     tag_name = tag_name.strip_suffix("_0").unwrap().to_string();
    // }
    let repo = Repository::open(repo).unwrap();
    let odb = repo.odb().unwrap();

    let mut unique_blobs = 0;
    let mut total_size = 0;
    let mut commits = 0;

    odb.foreach(|v| {
        if let Ok(blob) = repo.find_blob(*v) {
            unique_blobs += 1;
            total_size += blob.size();
        } else if let Ok(_) = repo.find_commit(*v) {
            commits += 1;
        }
        true
    })
    .unwrap();

    return RepoStatistics {
        unique_blobs,
        total_size,
        commits,
        path: repo.path().to_path_buf(),
    };
}

pub fn push(_strategy_info: String) {
    // // let reader = io::BufReader::new(File::open(strategy_file).unwrap());
    // // let strategy: PushStrategy = serde_json::from_reader(reader).unwrap();
    // let mut strategy: PushStrategy = serde_json::from_str(&strategy_info).unwrap();
    // // println!("[{}] Setting up push", strategy.tag_name);
    // let repo = Repository::open(&strategy.path).unwrap();
    // let mut remote = match repo.find_remote("origin") {
    //     Ok(r) => r,
    //     Err(_) => repo.remote("origin", "git@github.com:pypi-data/pypi-code.git").unwrap()
    // };
    //
    // let new_remote_callbacks = || {
    //     let mut callbacks = RemoteCallbacks::new();
    //     callbacks.push_update_reference(|r, status| {
    //         if let Some(s) = status {
    //             panic!("Reference {r} in {:?} could not be pushed: {s}", strategy.path)
    //         };
    //         println!("Reference {r} in {:?} pushed", strategy.tag_name);
    //         Ok(())
    //     });
    //     callbacks.push_transfer_progress(|current, total, bytes| {
    //         if current != 0 {
    //             println!("current: {current} total: {total} bytes: {bytes}");
    //         }
    //     });
    //     callbacks.credentials(|_, _, _| {
    //         // let p = Path::new("/Users/tom/.ssh/id_ed25519");
    //         // let p2 = Path::new("/Users/tom/.ssh/id_ed25519.pub");
    //         let p = Path::new("/root/.ssh/id_rsa");
    //         let p2 = Path::new("/root/.ssh/id_rsa.pub");
    //         Cred::ssh_key(
    //             "git",
    //             Some(p2),
    //             p,
    //             None,
    //         )
    //     });
    //
    //     callbacks
    // };
    //
    // remote.connect_auth(
    //     Direction::Push,
    //     Some(new_remote_callbacks()),
    //     None,
    // ).unwrap();
    //
    // let mut options = PushOptions::new();
    // options.remote_callbacks(new_remote_callbacks());
    //
    // let push_order = match strategy.push_order.as_slice() {
    //     [.., last] if last == &strategy.last_commit => {
    //         strategy.push_order
    //     }
    //     _ => {
    //         strategy.push_order.push(strategy.last_commit);
    //         strategy.push_order
    //     }
    // };
    //
    // println!("[{}] Starting pushing {} items", strategy.tag_name, push_order.len());
    // for commit in push_order {
    //     repo.set_head_detached(commit.parse().unwrap()).unwrap();
    //     println!("[{}] Pushing {commit}", strategy.tag_name);
    //     let refspec = format!("+HEAD:refs/tags/{}", strategy.tag_name);
    //     remote.push(&[refspec], Some(&mut options)).unwrap();
    //     println!("[{}] Pushed {commit}", strategy.tag_name);
    // }
}

//
// lazy_static! {
//     static ref INFO_REGEX: Regex =
//         Regex::new("[^\\s]* (?P<version>.*) \\((?P<filename>.*)\\)$").unwrap();
// }
//
// const TOTAL_FILES_PER_PUSH: i32 = 10_000;
// // Packages with more than this number of individual files are "big releases". For these we just
// //
// const BIG_RELEASE_THRESHOLD: i32 = 3_000;
// // How many "big release packages" to commit together?
// const BIG_RELEASE_RUNS: i32 = 10;
//
//
// fn ordered_commits(
//     package_name: String,
//     mut commits: Vec<Commit>,
//     repo: &Repository,
//     tag_name: String,
// ) -> PushStrategy {
//     commits.sort_by_cached_key(|v| v.time());
//     let mut total_files = 0;
//     let mut running_total = 0;
//     let mut push_order = vec![];
//     let last_commit = commits.last().unwrap().id().to_string();
//
//     let mut in_big_release_mode = false;
//     let mut big_release_run_count = 0;
//
//     for commit in commits {
//         if in_big_release_mode {
//             big_release_run_count += 1;
//             if big_release_run_count >= BIG_RELEASE_RUNS {
//                 push_order.push(commit.id().to_string());
//                 big_release_run_count = 0;
//             }
//             continue
//         }
//
//         let tree = commit.tree().unwrap();
//         let path = match serde_json::from_str::<CommitMessage>(commit.message().unwrap()) {
//             Ok(m) => PathBuf::from_str(&m.path).unwrap(),
//             Err(_) => {
//                 let res = INFO_REGEX.captures(commit.message().unwrap()).unwrap();
//                 let version = res.name("version").unwrap().as_str();
//                 let filename = res.name("filename").unwrap().as_str();
//                 let path = package_name_to_path(&package_name, version, filename);
//                 Path::new("code").join(path.0).join(path.1).join(path.2)
//             }
//         };
//         let p = tree.get_path(&path).unwrap_or_else(|_| panic!("Error unwrapping: {}", commit.message().unwrap()));
//         let sub_tree = p.to_object(repo).unwrap().into_tree().unwrap();
//         let mut tree_total = 0;
//         sub_tree
//             .walk(TreeWalkMode::PostOrder, |_, e| {
//                 match e.kind() {
//                     Some(t) if t == ObjectType::Blob => {
//                         tree_total += 1;
//                         running_total += 1;
//                         total_files += 1;
//                     }
//                     _ => {}
//                 };
//
//                 0
//             })
//             .unwrap();
//
//         if tree_total >= BIG_RELEASE_THRESHOLD {
//             in_big_release_mode = true;
//             push_order.push(commit.id().to_string());
//             continue;
//         }
//
//         if running_total >= TOTAL_FILES_PER_PUSH {
//             push_order.push(commit.id().to_string());
//             running_total = 0;
//         }
//     }
//
//     let path = repo.path().to_path_buf();
//     PushStrategy {
//         tag_name,
//         total_files,
//         last_commit,
//         push_order,
//         path,
//     }
// }
