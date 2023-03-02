use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use git2::{BranchType, Repository, Signature, Time};
use itertools::Itertools;
use crate::job::CommitMessage;
use tinytemplate::TinyTemplate;
use url::Url;

#[derive(serde::Serialize)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub uploaded_on: DateTime<Utc>,
}


pub fn create_index(repo_path: PathBuf, repo_url: Url) -> anyhow::Result<()> {
    let repo = Repository::open(&repo_path)?;
    let odb = repo.odb()?;
    let mut index = repo.index()?;

    let import_ref = repo.find_branch("import", BranchType::Local)?.into_reference().peel_to_commit()?;

    let mut walk = repo.revwalk()?;
    walk.push(import_ref.id())?;

    let mut packages: HashMap<String, Vec<_>> = HashMap::new();

    for commit in walk.into_iter() {
        let commit = repo.find_commit(commit?)?;
        let message: CommitMessage = serde_json::from_str(commit.message().unwrap())?;
        let time = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp_opt(commit.time().seconds(), 0).unwrap(), Utc);
        let entry = IndexEntry {
            name: message.name,
            version: message.version,
            path: message.path,
            uploaded_on: time,
        };
        match packages.entry(entry.name.to_string()) {
            Entry::Occupied(e) => {
                e.into_mut().push(entry)
            }
            Entry::Vacant(e) => {
                e.insert(vec![entry]);
            }
        }
    }

    let total_projects = packages.len();
    let total_releases = packages.values().flatten().count();
    let (min_release_time, max_release_time) = packages.values().flatten().map(|e| e.uploaded_on).minmax().into_option().unwrap();
    println!("Min: {min_release_time}");
    println!("Max: {max_release_time}");
    // println!("{} unique packages", packages.len());
    // println!("{} releases", packages.values().flatten().count());
    #[derive(serde::Serialize)]
    struct Context<'a> {
        first_release: NaiveDate,
        last_release: NaiveDate,
        total_projects: usize,
        total_releases: usize,
        table: Vec<(&'a String, usize)>,
        repo_url: Url,
    }

    let mut tt = TinyTemplate::new();
    tt.add_template("readme", include_str!("index_template.md"))?;
    let readme = tt.render("readme", &Context {
        first_release: min_release_time.date_naive(),
        last_release: max_release_time.date_naive(),
        total_projects,
        total_releases,
        repo_url,
        table: packages.iter().map(|(name, items)| {
            (name, items.len())
        }).sorted_by(|v1, v2| v1.1.cmp(&v2.1).reverse()).take(25).collect(),
    })?;
    println!("{readme}");

    let index_json = serde_json::to_string(&packages).unwrap();

    let readme_location = repo_path.join("README.md");
    let index_location = repo_path.join("index.json");
    fs::write(&readme_location, readme)?;
    fs::write(&index_location, index_json)?;

    index.add_path(&readme_location.strip_prefix(&repo_path)?)?;
    index.add_path(&index_location.strip_prefix(&repo_path)?)?;
    let tree_oid = index.write_tree()?;

    let signature = Signature::now(
        "Tom Forbes",
        "tom@tomforb.es",
    )?;

    let commit_oid = repo.commit(
        None,
        &signature,
        &signature,
        "Upload",
        &repo.find_tree(tree_oid)?,
        &[]
    )?;

    repo.branch("main", &repo.find_commit(commit_oid)?, true)?;

    Ok(())
}
