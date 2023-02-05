use std::fs;
use std::io::Write;
use std::path::PathBuf;
use git2::{Buf, RebaseOperationType, RebaseOptions, Repository, RepositoryInitOptions, Signature, Time};
use log::{info, warn};

pub fn combine(base_repo: PathBuf, target_repos: Vec<PathBuf>) {
    let opts = RepositoryInitOptions::new();
    let repo = Repository::init_opts(base_repo, &opts).unwrap();
    let object_db = repo.odb().unwrap();
    let mempack_backend = object_db.add_new_mempack_backend(3).unwrap();
    let mut repo_idx = repo.index().unwrap();
    repo_idx.set_version(4).unwrap();

    warn!("Fetching...");
    let commits_to_pick = target_repos.iter().enumerate().filter_map(|(idx, target)| {
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
        Some(repo.reference_to_annotated_commit(&reference).unwrap())
    });

    for (idx, reference) in commits_to_pick.enumerate() {
        warn!("Progress: {idx}/{}", target_repos.len() - 1);
        info!("Rebasing from {}", reference.refname().unwrap());
        let local_ref = match repo.head() {
            //repo.find_branch("merge", BranchType::Local) {
            Ok(v) => repo.reference_to_annotated_commit(&v).unwrap(),
            Err(_) => {
                repo.set_head_detached(reference.id()).unwrap();
                continue;
            }
        };
        info!("Rebasing commit onto: {}", local_ref.id());

        let mut opts = RebaseOptions::new();
        opts.inmemory(true);
        let mut rebase = repo
            .rebase(
                Some(&reference),
                Option::from(&local_ref),
                None,
                Some(&mut opts),
            )
            .unwrap();
        let signature =
            Signature::new("Tom Forbes", "tom@tomforb.es", &Time::new(0, 0)).unwrap();

        let mut last_commit = None;
        while let Some(x) = rebase.next() {
            let kind = x.unwrap().kind().unwrap();
            match kind {
                RebaseOperationType::Pick => {
                    last_commit = Some(rebase.commit(None, &signature, None).unwrap());
                }
                _ => {
                    panic!("unknown rebase kind {kind:?}");
                }
            }
        }
        rebase.finish(None).unwrap();
        let new_idx = rebase.inmemory_index().unwrap();
        for item in new_idx.iter() {
            repo_idx.add(&item).unwrap();
        }
        let last_commit = repo.find_commit(last_commit.unwrap()).unwrap();

        repo.set_head_detached(last_commit.id()).unwrap();
    }

    warn!("Rebase done, resetting head");
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("master", &head, true).unwrap();

    warn!("Dumping packfile");
    let mut buf = Buf::new();
    mempack_backend.dump(&repo, &mut buf).unwrap();

    let mut writer = object_db.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();
    warn!("Writing index");
    repo_idx.write().unwrap();
}