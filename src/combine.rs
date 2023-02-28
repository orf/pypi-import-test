use git2::{FileMode, ObjectType, Repository};

use std::fs;
use std::io::Write;

use crate::job::CommitMessage;
use crate::utils::log_timer;
use anyhow::Context;

use git2::build::TreeUpdateBuilder;
use indicatif::ProgressIterator;
use itertools::Itertools;
use std::path::PathBuf;

pub fn merge_all_branches(into: PathBuf, mut repos: Vec<PathBuf>) -> anyhow::Result<()> {
    let target_repo = Repository::init(&into)?;
    let target_pack_dir = target_repo.path().join("objects").join("pack");
    let _target_head_dir = target_repo.path().join("refs").join("heads");

    let into_file_name = into.file_name().unwrap().to_str().unwrap();

    // Ensure repos are sorted so that tags are somewhat deterministic
    repos.sort();

    // let mut branches = vec![];

    let previous = log_timer("Starting", into_file_name, None);

    // Step 1: Create a branch in each target repo and copy the data. this is faster than using a remote.
    for (_idx, repo) in repos.iter().enumerate() {
        println!("{}", repo.display());
        let repo = match Repository::open(repo) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let pack_dir = repo.path().join("objects").join("pack");
        for item in fs::read_dir(pack_dir)? {
            let item = item?;
            let to_path = target_pack_dir.join(item.file_name());
            if !to_path.exists() {
                fs::copy(item.path(), &to_path).with_context(|| {
                    format!(
                        "Error copying {} to {}",
                        item.path().display(),
                        to_path.display()
                    )
                })?;
            }
        }
    }

    let odb = target_repo.odb()?;
    let mempack_backend = odb.add_new_mempack_backend(3)?;

    let mut commits = vec![];
    odb.foreach(|v| {
        let (_, obj_type) = odb.read_header(*v).unwrap();
        if obj_type == ObjectType::Commit {
            commits.push(*v);
        }
        true
    })?;

    let commits: Vec<_> = commits
        .into_iter()
        .map(|oid| target_repo.find_commit(oid).unwrap())
        .sorted_by(|c1, c2| c1.time().cmp(&c2.time()))
        .collect();

    println!("Got commits: {}", commits.len());

    let mut head_tree = target_repo.find_tree(target_repo.treebuilder(None)?.write()?)?;

    let mut parent_commit = None;

    for commit in commits.into_iter().progress() {
        let commit_message = commit.message().unwrap();
        let message: CommitMessage = serde_json::from_str(commit_message)
            .with_context(|| format!("Message: {}", commit.message().unwrap()))?;

        // let new_path = format!("{}/{}", message.name, message.file);

        // let tree_path = Path::new(&message.path);
        let commit_tree = commit.tree()?;

        let mut builder = TreeUpdateBuilder::new();
        builder.upsert(message.path, commit_tree.id(), FileMode::Tree);
        let updated_oid = builder.create_updated(&target_repo, &head_tree).unwrap();
        head_tree = target_repo.find_tree(updated_oid)?;

        let parent_commit_oid = match &parent_commit {
            // I don't know how to generalise this :(
            None => target_repo.commit(
                None,
                &commit.author(),
                &commit.committer(),
                commit_message,
                &head_tree,
                &[],
            )?,
            Some(c) => target_repo.commit(
                None,
                &commit.author(),
                &commit.committer(),
                commit_message,
                &head_tree,
                &[c],
            )?,
        };
        parent_commit = Some(target_repo.find_commit(parent_commit_oid)?)
    }

    target_repo
        .branch("imported", &parent_commit.unwrap(), true)
        .unwrap();

    let mut buf = git2::Buf::new();
    mempack_backend.dump(&target_repo, &mut buf).unwrap();
    mempack_backend.reset().unwrap();

    log_timer("Writing", into_file_name, previous);

    let mut writer = odb.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();

    // let mut repo_tree_builder = target_repo.treebuilder(Some(&head_tree)).unwrap();

    // let package_name_tree = match head_tree.get_path(message.name.as_ref()) {
    //     Ok(e) => {
    //         target_repo.find_tree(e.id())?
    //     }
    //     Err(e) if e.code() == ErrorCode::NotFound => {
    //         let mut new_tree = target_repo.treebuilder(None).unwrap();
    //         new_tree.insert(
    //             package_file_name,
    //             commit_tree.id(),
    //             0o040000
    //         ).unwrap();
    //         target_repo.find_tree(
    //             new_tree.write()?
    //         )?
    //     },
    //     Err(e) => {
    //         panic!("Unknown git error: {e}")
    //     }
    // };

    // repo_tree_builder.insert(
    //     format!("{new_path}/foo"),
    //     commit_tree.id(),
    //     0o040000
    // )?;
    // repo_tree_builder.write().unwrap();

    // repo_tree_builder
    //     .insert(
    //         tree_path
    //             .to_str()
    //             .unwrap()
    //             .strip_prefix("packages/")
    //             .unwrap()
    //             .strip_suffix('/')
    //             .unwrap(),
    //         item.id(),
    //         0o040000,
    //     )
    //     .unwrap();
    // let package_tree_oid = repo_tree_builder.write().unwrap();
    // // repo_tree_builder
    // //     .insert("packages", package_tree_oid, 0o040000)
    // //     .unwrap();
    // // let repo_tree_oid = repo_tree_builder.write().unwrap();
    // head_tree = target_repo.find_tree(package_tree_oid).unwrap();

    // parent_commit = target_repo.find_commit(parent_commit_oid).unwrap();
    // }

    // let merged_reference = target_repo
    //     .branch("merged", &target_repo.head()?.peel_to_commit()?, true)?
    //     .into_reference();
    //
    // let mut head_tree = target_repo.head().unwrap().peel_to_tree().unwrap();
    // let mut parent_commit = target_repo.head().unwrap().peel_to_commit().unwrap();
    //
    // for branch in branches {
    //     previous = log_timer("Branch", into_file_name, previous);
    //     let branch = target_repo.find_branch(&branch, BranchType::Local)?;
    //     let branch_commit = target_repo.reference_to_annotated_commit(&branch.into_reference())?;
    //     let mut revwalk = target_repo.revwalk()?;
    //     revwalk.push(branch_commit.id())?;
    //     revwalk.hide_head()?;
    //
    //     for commit_oid in revwalk {
    //         let commit_oid = commit_oid?;
    //         let commit = target_repo.find_commit(commit_oid)?;
    //     }
    // }

    //
    // let ordered_commits: Vec<_> = ordered_commits
    //     .into_iter()
    //     .unique_by(|(path, _, _)| path.clone())
    //     .collect();
    //
    // let results: Vec<_> = ordered_commits.into_iter().map(|(path, package_oid, commit)| {
    //     (new_oid, commit)
    // }).collect();
    //
    // previous = log_timer("Commits", into_file_name, previous);
    //

    // let mut parent_commit = merged_reference.peel_to_commit()?;
    // // let mut head_tree_oid = merged_reference.peel_to_tree()?.id();
    //
    // // Ok, now we commit!
    // for (new_tree_oid, commit_oid) in results {
    //     let prev_commit = target_repo.find_commit(commit_oid)?;
    //     let message = prev_commit.message().unwrap();
    //     let tree_oid = target_repo.find_tree(new_tree_oid)?;

    // }
    //
    // previous = log_timer("Saving", into_file_name, previous);
    //

    //
    // log_timer("Done", into_file_name, previous);

    // (0..ordered_commits.len()).into_par_iter().fold_chunks(
    //     10_000,
    //     || {
    //         (repo, None::<TreeUpdateBuilder>, tree)
    //     },
    //     |(repo, update_builder, previous_tree), idx| {
    //         let update_builder = match update_builder {
    //             None => {
    //                 let mut update = TreeUpdateBuilder::new();
    //                 for (p, oid, _) in ordered_commits[0..idx].iter() {
    //                     update.upsert(p, *oid, FileMode::Tree);
    //                 }
    //                 update
    //             }
    //             Some(mut update) => {
    //                 let (p, oid, _) = &ordered_commits[idx];
    //                 update.upsert(p, *oid, FileMode::Tree);
    //                 update
    //             }
    //         };
    //
    //         // update_builder.create_updated(&repo, )
    //
    //         (repo, Some(update_builder), previous_tree)
    //     },
    // ).for_each(|v| {
    //     println!("Shit son");
    // });

    //
    // use indicatif::ProgressIterator;
    //
    // let mut update = TreeUpdateBuilder::new();
    // for (fuck, fuck2, fuck3) in ordered_commits[0..ordered_commits.len()-1].iter() {
    //     update.upsert(fuck, *fuck2, FileMode::Tree);
    // }
    //
    // let head_tree = target_repo.find_tree(head_tree_oid).unwrap();
    // warn!("Current head tree OID {}", head_tree.id());
    // let x1 = Instant::now();
    // let head_tree_oid = update.create_updated(&target_repo, &head_tree).unwrap();
    // warn!("New head tree OID {}. MS: {}", head_tree_oid, x1.elapsed().as_millis());
    //
    // let mut update = TreeUpdateBuilder::new();
    // for (fuck, fuck2, fuck3) in ordered_commits.iter() {
    //     update.upsert(fuck, *fuck2, FileMode::Tree);
    // }
    //
    // let head_tree = target_repo.find_tree(head_tree_oid).unwrap();
    // warn!("Current head tree OID {}", head_tree.id());
    // let x1 = Instant::now();
    // let head_tree_oid = update.create_updated(&target_repo, &head_tree).unwrap();
    // warn!("New head tree OID {}. MS: {}", head_tree_oid, x1.elapsed().as_millis());

    // let mut update = TreeUpdateBuilder::new();
    // for (fuck, fuck2, fuck3) in ordered_commits.iter() {
    //     update.upsert(fuck, *fuck2, FileMode::Tree);
    // }
    //
    // warn!("Current head tree OID {}", og_head_tree.id());
    // let x1 = Instant::now();
    // let head_tree_oid = update.create_updated(&target_repo, &og_head_tree).unwrap();
    // warn!("New head tree OID {}. MS: {}", head_tree_oid, x1.elapsed().as_millis());

    //
    // (0..1000).into_par_iter().progress_count(1000).for_each_init(|| {
    //     let repo = Repository::open(&into).unwrap();
    //     {
    //         let odb = repo.odb().unwrap();
    //         odb.add_new_mempack_backend(3).unwrap();
    //     }
    //     repo.set_odb(&odb).unwrap();
    //     repo
    // }, |repo, v| {
    //     let mut update = TreeUpdateBuilder::new();
    //     for (fuck, fuck2, fuck3) in ordered_commits[0..ordered_commits.len() - 3].iter() {
    //         update.upsert(fuck, *fuck2, FileMode::Tree);
    //     }
    //     drop(update);
    //     // warn!("Saving");
    //     // let head_tree = repo.find_tree(head_tree_oid).unwrap();
    //     // let head_tree_oid = update.create_updated(&repo, &head_tree).unwrap();
    //     // warn!("OID {head_tree_oid}");
    // });

    // let mut update = TreeUpdateBuilder::new();
    // for (fuck, fuck2, fuck3) in ordered_commits.iter() {
    //     update.upsert(fuck, *fuck2, FileMode::Tree);
    // }
    //
    // warn!("Saving");
    // let head_tree = target_repo.find_tree(head_tree_oid).unwrap();
    // let fuck_head_tree_oid = update.create_updated(&target_repo, &head_tree).unwrap();
    // warn!("OID {head_tree_oid}");

    // let mut all_tree_data: Vec<_> = indexes.into_par_iter().progress_with(pbar).map_init(|| {
    //     let repo = Repository::open(&into).unwrap();
    //     {
    //         let odb = repo.odb().unwrap();
    //         odb.add_new_mempack_backend(3).unwrap();
    //     }
    //     repo.set_odb(&odb).unwrap();
    //     repo
    // }, |repo, idx| {
    //     let slice = &ordered_commits[idx - step_by..idx];
    //
    //     let mut head_tree_oid = head_tree_oid;
    //
    //     let mut update = TreeUpdateBuilder::new();
    //     slice.iter().enumerate().map(|(inner_idx, (tree_path, oid, commit))| {
    //         let head_tree = repo.find_tree(head_tree_oid).unwrap();
    //         for (tree_path, oid, commit) in slice {
    //             // println!("[{idx}] Adding {}", tree_path.display());
    //             update.upsert(tree_path, *oid, FileMode::Tree);
    //         }
    //         head_tree_oid = update.create_updated(repo, &head_tree).unwrap();
    //         (idx + inner_idx, head_tree_oid)
    //     }).collect::<Vec<_>>()
    // }).collect();

    // let all_tree_data: Vec<_> = all_tree_data.into_iter().flatten().sorted_by_key(|(idx, _)| *idx).collect();
    //
    // println!("New oids: {}", all_tree_data.len());

    // previous = log_timer("Trees", into_file_name, previous);

    // for branch_name in iterator {
    //     let make_revwalk = || -> anyhow::Result<Revwalk> {
    //         Ok(revwalk)
    //     };
    //
    //     let foo = Instant::now();
    //     // let walked: Vec<_> = iter.collect();
    //
    //     let total = walked.len() as f64;
    //     println!("Total {total}. Fetched in {}", foo.elapsed().as_secs_f64());
    //
    //     let mut tree_data = vec![];
    //
    //     let start_time = Instant::now();
    //     for (iter_idx, commit_oid) in walked.into_iter().enumerate() {
    //         let s_start = Instant::now();

    //     }
    //
    //     #[cfg(feature = "no_progress")]
    //     {
    //         previous = log_timer("tree_data", into_file_name, previous);
    //     }
    //
    //     let repo_lock = Mutex::new(target_repo);
    //

    //
    //     println!("Tree data len: {}", all_tree_data.len());
    //     // let mut update = TreeUpdateBuilder::new();
    //     // update.upsert(tree_path, item.id(), FileMode::Tree);
    //     //
    //     // let i_commit = Instant::now();
    //     // let tree_oid = update
    //     //     .create_updated(&target_repo, &head_tree)?;
    //

    //     // let i_end = i_commit.elapsed().as_secs_f32();
    //     //
    //     // parent_commit = target_repo.find_commit(parent_commit_oid)?;
    //
    //     // println!("[{iter_idx}/{total}] Total time: {:0.4} / Start: {s_end:0.4} / Tree: {i_end:0.4}", s_start.elapsed().as_secs_f32());
    //     // }
    //
    //     let total_time = start_time.elapsed().as_secs_f64();
    //     println!("Finished iteration in {} ({})", total_time / total, total_time as u64);
    //
    //     #[cfg(feature = "no_progress")]
    //     {
    //         previous = log_timer("looped", into_file_name, previous);
    //     }
    //
    //     target_repo.find_branch(branch_name, BranchType::Local)?.delete()?;
    //
    //     // let mut buf = Buf::new();
    //     // mempack_backend.dump(&target_repo, &mut buf).unwrap();
    //     //
    //     // #[cfg(feature = "no_progress")]
    //     // {
    //     //     previous = log_timer("dumped", into_file_name, previous);
    //     // }
    //     //
    //     // let mut writer = odb.packwriter().unwrap();
    //     // writer.write_all(&buf).unwrap();
    //     // writer.commit().unwrap();
    //     //
    //     // #[cfg(feature = "no_progress")]
    //     // {
    //     //     previous = log_timer("committed", into_file_name, previous);
    //     // }
    //     //
    //     // mempack_backend.reset()?;
    // }

    Ok(())
}
