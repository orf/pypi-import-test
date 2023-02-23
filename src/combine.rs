use git2::build::TreeUpdateBuilder;
use git2::{FileMode, ObjectType, Repository, Signature, Sort, Time, TreeWalkMode};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use chrono::Utc;
use log::warn;
use std::path::PathBuf;

pub fn merge_all_branches(repo_path: PathBuf, branch_name: String) -> anyhow::Result<()> {
    let repo = Repository::open(repo_path)?;

    let all_branches: Vec<_> = repo
        .branches(None)?
        .flatten()
        .filter_map(|(b, _)| match b.name().unwrap().unwrap() {
            "master" | "main" | "origin/HEAD" => None,
            _ => Some(b),
        })
        .collect();

    let _all_commits: Vec<_> = all_branches
        .iter()
        .map(|b| b.get().peel_to_commit().unwrap())
        .collect();

    let head_tree = repo.head().unwrap().peel_to_tree().unwrap();
    let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
    let _builder = repo.treebuilder(Some(&head_tree)).unwrap();
    // let base_tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let mut update = TreeUpdateBuilder::new();

    // I messed up and there are duplicates. This works around that.
    let mut workaround: HashMap<String, u16> = HashMap::new();

    all_branches.iter().for_each(|b| {
        // println!("New branch");
        let mut walk2 = repo.revwalk().unwrap();
        walk2.push(b.get().peel_to_commit().unwrap().id()).unwrap();
        walk2.set_sorting(Sort::REVERSE).unwrap();
        walk2.hide_head().unwrap();
        match walk2.into_iter().last() {
            None => {
                println!("NONE?")
            }
            Some(v) => {
                let last = v.unwrap();
                let commit_tree = repo.find_commit(last).unwrap().tree().unwrap();
                commit_tree
                    .walk(TreeWalkMode::PreOrder, |x, y| {
                        // code/adb3/1.1.0/tar.gz/ -> 4 splits.
                        if let (4, Some(ObjectType::Tree)) = (x.split('/').count(), y.kind()) {
                            let mut name = format!("{}{}", x, y.name().unwrap());
                            match workaround.entry(name.clone()) {
                                Entry::Occupied(mut v) => {
                                    v.insert(v.get() + 1);
                                    name = format!("{}{}{}", x, v.get(), y.name().unwrap());
                                }
                                Entry::Vacant(v) => {
                                    v.insert(0);
                                }
                            }
                            update.upsert(name, y.id(), FileMode::Tree);
                            return 1;
                        }
                        0
                    })
                    .unwrap();
            }
        }

        // for item in walk2 {
        //     println!("Item: {:?}", item);
        // }
    });

    warn!("Merging {} commits", all_branches.len());

    // for commit in &all_commits {
    //     // Combine all trees into a single treebuilder.
    //     commit
    //         .tree()
    //         .unwrap()
    // }
    warn!("Creating tree");
    let base_tree = update.create_updated(&repo, &head_tree).unwrap();
    let base_tree = repo.find_tree(base_tree).unwrap();
    warn!("Created tree {}", base_tree.id());

    let time_now = Utc::now();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(time_now.timestamp(), 0),
    )
    .unwrap();

    let parent_commits = vec![&head_commit];
    // let mut parent_commits: Vec<_> = all_commits.iter().collect();
    // parent_commits.insert(0, &head_commit);

    let commit = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Merging partitions",
            &base_tree,
            &parent_commits,
        )
        .unwrap();
    let commit = repo.find_commit(commit).unwrap();

    warn!("Committed, setting branch");

    let mut repo_idx = repo.index().unwrap();
    let _head_ref = repo.branch(&branch_name, &commit, true).unwrap();

    warn!("Deleting branches");

    for mut branch in all_branches {
        match branch.delete() {
            Ok(_) => {}
            Err(e) => {
                warn!("Error deleting {}: {e}", branch.name().unwrap().unwrap());
            }
        }
    }

    warn!("Writing index");

    repo_idx.write().unwrap();

    Ok(())
}
