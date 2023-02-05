use chrono::Utc;
use git2::build::TreeUpdateBuilder;
use git2::{
    FileMode, ObjectType, Repository, RepositoryInitOptions, Signature, Time, TreeWalkMode,
};
use log::warn;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub fn combine(base_repo: PathBuf, target_repos: Vec<PathBuf>) {
    let opts = RepositoryInitOptions::new();
    let repo = Repository::init_opts(base_repo, &opts).unwrap();
    let mut repo_idx = repo.index().unwrap();
    repo_idx.set_version(4).unwrap();

    let time_now = Utc::now();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(time_now.timestamp(), 0),
    )
    .unwrap();

    warn!("Fetching...");
    let references_to_merge: Vec<_> = target_repos
        .into_iter()
        .enumerate()
        .filter_map(|(idx, target)| {
            let target = fs::canonicalize(target).unwrap();
            let remote_name = format!("import_{idx}");
            let _ = repo.remote_delete(&remote_name);
            let mut remote = repo
                .remote(
                    &remote_name,
                    format!("file://{}", target.to_str().unwrap()).as_str(),
                )
                .unwrap();
            warn!("Fetching remote {}", remote.url().unwrap());
            if let Err(e) = remote.fetch(
                &[format!(
                    "refs/heads/master:refs/remotes/{remote_name}/master"
                )],
                None,
                None,
            ) {
                warn!("Error fetching remote: {}", e);
                return None;
            }
            let reference = repo
                .find_reference(format!("refs/remotes/{remote_name}/master").as_str())
                .unwrap();
            Some(reference.peel_to_commit().unwrap())
        })
        .collect();

    let total = references_to_merge.len();
    warn!("Merging {} references", total);

    let builder = repo.treebuilder(None).unwrap();
    let base_tree = repo.find_tree(builder.write().unwrap()).unwrap();
    let mut update = TreeUpdateBuilder::new();

    for (idx, item) in references_to_merge.iter().enumerate() {
        warn!("Merging tree {}/{}", idx, total);
        item.tree()
            .unwrap()
            .walk(TreeWalkMode::PreOrder, |x, y| {
                if let Some(ObjectType::Blob) = y.kind() {
                    update.upsert(
                        format!("{}{}", x, y.name().unwrap()),
                        y.id(),
                        FileMode::Blob,
                    );
                }
                0
            })
            .unwrap();
    }

    warn!("Creating tree");
    let base_tree = update.create_updated(&repo, &base_tree).unwrap();
    let base_tree = repo.find_tree(base_tree).unwrap();

    warn!("Finished merging trees, committing");
    let parent_commits: Vec<_> = references_to_merge.iter().collect();

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "test",
        &base_tree,
        &parent_commits,
    )
    .unwrap();

    warn!("Writing index");
    repo_idx.write().unwrap();
}
