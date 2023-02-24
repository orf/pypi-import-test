use git2::build::TreeUpdateBuilder;
use git2::{BranchType, FileMode, Repository, Revwalk};

use std::fs;

use crate::job::CommitMessage;
use crate::utils::set_pbar_options;
use anyhow::Context;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressIterator};
use std::path::{Path, PathBuf};

pub fn merge_all_branches(into: PathBuf, mut repos: Vec<PathBuf>) -> anyhow::Result<()> {
    let target_repo = Repository::clone("file:///Users/tom/tmp/combined/pypi-code/", &into)?;
    let target_pack_dir = target_repo.path().join("objects").join("pack");
    let target_head_dir = target_repo.path().join("refs").join("heads");

    let odb = target_repo.odb()?;
    let _mempacked_backend = odb.add_new_mempack_backend(3)?;

    // Ensure repos are sorted so that tags are somewhat deterministic
    repos.sort();

    let mut branches = vec![];

    // Step 1: Create a branch in each target repo and copy the data. this is faster than using a remote.
    for (idx, repo) in repos.iter().enumerate() {
        let repo = Repository::open(repo)?;
        let head = repo.head()?.peel_to_commit()?;
        let branch_name = format!("import_{idx}");
        repo.branch(&branch_name, &head, true)?;

        let pack_dir = repo.path().join("objects").join("pack");
        for item in fs::read_dir(pack_dir)? {
            let item = item?;
            fs::copy(item.path(), target_pack_dir.join(item.file_name()))?;
        }

        fs::write(target_head_dir.join(&branch_name), head.id().to_string())?;

        branches.push(branch_name);
    }

    // Stitch them all together!

    let multi_progress = MultiProgress::new();
    multi_progress.set_draw_target(ProgressDrawTarget::stderr_with_hz(1));

    let progress = set_pbar_options(
        multi_progress.add(ProgressBar::new(branches.len() as u64)),
        "merging branches",
        false,
    );

    let merged_reference = target_repo
        .branch("merged", &target_repo.head()?.peel_to_commit()?, true)?
        .into_reference();
    let mut parent_commit = merged_reference.peel_to_commit()?;

    let mut head_tree = merged_reference.peel_to_tree()?;

    let mut update = TreeUpdateBuilder::new();

    for branch in branches.iter().progress_with(progress) {
        let branch = target_repo.find_branch(branch, BranchType::Local)?;

        let branch_commit = target_repo.reference_to_annotated_commit(&branch.into_reference())?;

        let make_revwalk = || -> anyhow::Result<Revwalk> {
            let mut revwalk = target_repo.revwalk()?;
            revwalk.push(branch_commit.id())?;
            revwalk.hide_head()?;
            Ok(revwalk)
        };

        let total_revisions = make_revwalk()?.into_iter().count();
        let pbar = set_pbar_options(
            multi_progress.add(ProgressBar::new(total_revisions as u64)),
            "committing",
            false,
        );

        for commit_oid in make_revwalk()?.into_iter().progress_with(pbar) {
            let commit_oid = commit_oid?;
            let commit = target_repo.find_commit(commit_oid)?;
            let commit_message = commit.message().unwrap();
            let message: CommitMessage = serde_json::from_str(commit_message)
                .with_context(|| format!("Message: {}", commit.message().unwrap()))?;
            let tree_path = Path::new(&message.path);
            let commit_tree = commit.tree()?;
            let item = commit_tree.get_path(tree_path)?;

            update.upsert(tree_path, item.id(), FileMode::Tree);

            let tree_oid = update
                .create_updated(&target_repo, &head_tree)
                .with_context(|| format!("Duplicate?? {}", message.path))?;

            head_tree = target_repo.find_tree(tree_oid)?;

            let parent_commit_oid = target_repo.commit(
                merged_reference.name(),
                &commit.author(),
                &commit.committer(),
                commit_message,
                &head_tree,
                &[&parent_commit],
            )?;
            parent_commit = target_repo.find_commit(parent_commit_oid)?;
        }
    }

    // all_branches.iter().for_each(|b| {
    //     let mut walk2 = repo.revwalk().unwrap();
    //     walk2.push(b.get().peel_to_commit().unwrap().id()).unwrap();
    //     walk2.set_sorting(Sort::REVERSE).unwrap();
    //     walk2.hide_head().unwrap();
    //     match walk2.into_iter().last() {
    //         None => {
    //             println!("NONE?")
    //         }
    //         Some(v) => {
    //             let last = v.unwrap();
    //             let commit_tree = repo.find_commit(last).unwrap().tree().unwrap();
    //             commit_tree
    //                 .walk(TreeWalkMode::PreOrder, |x, y| {
    //                     // code/adb3/1.1.0/tar.gz/ -> 4 splits.
    //                     if let (4, Some(ObjectType::Tree)) = (x.split('/').count(), y.kind()) {
    //                         let mut name = format!("{}{}", x, y.name().unwrap());
    //                         match workaround.entry(name.clone()) {
    //                             Entry::Occupied(mut v) => {
    //                                 v.insert(v.get() + 1);
    //                                 name = format!("{}{}{}", x, v.get(), y.name().unwrap());
    //                             }
    //                             Entry::Vacant(v) => {
    //                                 v.insert(0);
    //                             }
    //                         }
    //                         update.upsert(name, y.id(), FileMode::Tree);
    //                         return 1;
    //                     }
    //                     0
    //                 })
    //                 .unwrap();
    //         }
    //     }
    // }

    // let all_branches: Vec<_> = repo
    //     .branches(None)?
    //     .flatten()
    //     .filter_map(|(b, _)| match b.name().unwrap().unwrap() {
    //         "master" | "main" | "origin/HEAD" => None,
    //         _ => Some(b),
    //     })
    //     .collect();
    //
    // let _all_commits: Vec<_> = all_branches
    //     .iter()
    //     .map(|b| b.get().peel_to_commit().unwrap())
    //     .collect();
    //
    //
    // let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
    // let _builder = repo.treebuilder(Some(&head_tree)).unwrap();
    // // let base_tree = repo.find_tree(builder.write().unwrap()).unwrap();
    // let mut update = TreeUpdateBuilder::new();
    //
    // // I messed up and there are duplicates. This works around that.
    // let mut workaround: HashMap<String, u16> = HashMap::new();
    //

    //
    //     // for item in walk2 {
    //     //     println!("Item: {:?}", item);
    //     // }
    // });
    //
    // warn!("Merging {} commits", all_branches.len());
    //
    // // for commit in &all_commits {
    // //     // Combine all trees into a single treebuilder.
    // //     commit
    // //         .tree()
    // //         .unwrap()
    // // }
    // warn!("Creating tree");
    // let base_tree = update.create_updated(&repo, &head_tree).unwrap();
    // let base_tree = repo.find_tree(base_tree).unwrap();
    // warn!("Created tree {}", base_tree.id());
    //
    // let time_now = Utc::now();
    // let signature = Signature::new(
    //     "Tom Forbes",
    //     "tom@tomforb.es",
    //     &Time::new(time_now.timestamp(), 0),
    // )
    // .unwrap();
    //
    // let parent_commits = vec![&head_commit];
    // // let mut parent_commits: Vec<_> = all_commits.iter().collect();
    // // parent_commits.insert(0, &head_commit);
    //
    // let commit = repo
    //     .commit(
    //         Some("HEAD"),
    //         &signature,
    //         &signature,
    //         "Merging partitions",
    //         &base_tree,
    //         &parent_commits,
    //     )
    //     .unwrap();
    // let commit = repo.find_commit(commit).unwrap();
    //
    // warn!("Committed, setting branch");
    //
    // let mut repo_idx = repo.index().unwrap();
    // let _head_ref = repo.branch(&branch_name, &commit, true).unwrap();
    //
    // warn!("Deleting branches");
    //
    // for mut branch in all_branches {
    //     match branch.delete() {
    //         Ok(_) => {}
    //         Err(e) => {
    //             warn!("Error deleting {}: {e}", branch.name().unwrap().unwrap());
    //         }
    //     }
    // }
    //
    // warn!("Writing index");
    //
    // repo_idx.write().unwrap();

    Ok(())
}
