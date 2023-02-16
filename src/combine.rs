// use chrono::Utc;
// use git2::build::TreeUpdateBuilder;
// use git2::{
//     FileMode, ObjectType, Repository, RepositoryInitOptions, Signature, Time, TreeWalkMode,
// };
// use log::warn;
// use rayon::prelude::*;
// use std::fs;
// use std::path::PathBuf;
//
// pub fn combine(job_idx: usize, base_repo: PathBuf, target_repos: Vec<PathBuf>) {
//     let opts = RepositoryInitOptions::new();
//     let repo = Repository::init_opts(base_repo, &opts).unwrap();
//     let mut repo_idx = repo.index().unwrap();
//     repo_idx.set_version(4).unwrap();
//
//     let time_now = Utc::now();
//     let signature = Signature::new(
//         "Tom Forbes",
//         "tom@tomforb.es",
//         &Time::new(time_now.timestamp(), 0),
//     )
//     .unwrap();
//
//     warn!("[{}] Adding remotes...", job_idx);
//     let mut remotes: Vec<_> = target_repos
//         .iter()
//         .enumerate()
//         .map(|(idx, target)| {
//             let target = fs::canonicalize(target).unwrap();
//             let remote_name = format!("import_{idx}");
//             let _ = repo.remote_delete(&remote_name);
//             let remote = repo
//                 .remote(
//                     &remote_name,
//                     format!("file://{}", target.to_str().unwrap()).as_str(),
//                 )
//                 .unwrap();
//             (remote_name, remote)
//         })
//         .collect();
//
//     warn!("[{}] Fetching remotes...", job_idx);
//     let remotes: Vec<_> = remotes
//         .par_iter_mut()
//         .map(|(remote_name, remote)| {
//             remote
//                 .fetch(
//                     &[format!(
//                         "refs/heads/master:refs/remotes/{remote_name}/master"
//                     )],
//                     None,
//                     None,
//                 )
//                 .unwrap(); // To-do: handle errors
//                            // warn!("[{}] Fetched remote", job_idx);
//             remote_name
//         })
//         .collect();
//
//     let commits: Vec<_> = remotes
//         .into_iter()
//         .flat_map(|name| {
//             match repo.find_reference(format!("refs/remotes/{name}/master").as_str()) {
//                 Ok(r) => Some(r.peel_to_commit().unwrap()),
//                 Err(_) => None,
//             }
//         })
//         .collect();
//
//     let total = commits.len();
//     warn!("[{}] Merging {} remotes", job_idx, total);
//
//     let builder = repo.treebuilder(None).unwrap();
//     let base_tree = repo.find_tree(builder.write().unwrap()).unwrap();
//     let mut update = TreeUpdateBuilder::new();
//
//     for commit in &commits {
//         // Combine all trees into a single treebuilder.
//         commit
//             .tree()
//             .unwrap()
//             .walk(TreeWalkMode::PreOrder, |x, y| {
//                 // code/adb3/1.1.0/tar.gz/ -> 4 splits.
//                 if let (4, Some(ObjectType::Tree)) = (x.split('/').count(), y.kind()) {
//                     update.upsert(
//                         format!("{}{}", x, y.name().unwrap()),
//                         y.id(),
//                         FileMode::Tree,
//                     );
//                     return 1;
//                 }
//                 0
//             })
//             .unwrap();
//     }
//
//     warn!("[{}] Creating tree", job_idx);
//     let base_tree = update.create_updated(&repo, &base_tree).unwrap();
//     let base_tree = repo.find_tree(base_tree).unwrap();
//
//     warn!("[{}] Finished merging trees, committing", job_idx);
//     let parent_commits: Vec<_> = commits.iter().collect();
//
//     repo.commit(
//         Some("HEAD"),
//         &signature,
//         &signature,
//         "Merging partitions",
//         &base_tree,
//         &parent_commits,
//     )
//     .unwrap();
//
//     warn!("[{}] Writing index", job_idx);
//     repo_idx.write().unwrap();
//     warn!("[{}] Finished", job_idx);
// }
