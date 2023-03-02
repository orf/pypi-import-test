use crate::job::APP_USER_AGENT;
use serde::{Deserialize, Serialize};
use std::{env, fs};
use std::path::PathBuf;
use anyhow::Context;
use git2::{BranchType, Error, ErrorCode, Remote, Repository};
use crate::combine::JsonIndex;

#[derive(Debug, Serialize)]
struct NewRepo {
    name: String,
    description: String,
    private: bool,
}

#[derive(Debug, Deserialize)]
struct CreatedRepo {
    ssh_url: String
}

pub fn create_repository(repo_path: PathBuf) -> anyhow::Result<()> {
    let repo = Repository::open(&repo_path)?;
    repo.set_head("refs/heads/main")?;
    repo.checkout_head(None)?;

    let index_json = fs::read_to_string(repo_path.join("index.json"))?;
    let index_json: JsonIndex = serde_json::from_str(&index_json)?;

    // let repo_name = format!("pypi-code-{}", repo_path.file_name().unwrap().to_str().unwrap());
    let repo_name = index_json.url.path().split('/').last().unwrap();

    // Get the GitHub token from the environment variable
    let token = env::var("IMPORT_GITHUB_TOKEN")?;

    // Set up the API request
    let org = "pypi-data";
    let url = format!("https://api.github.com/orgs/{}/repos", org);
    let args = NewRepo {
        name: repo_name.to_string(),
        description: format!("PyPi code from {} to {}", index_json.earliest_release.date_naive(), index_json.latest_release.date_naive()),
        private: true,
    };
    // Send the API request to create the new repository
    let response = ureq::post(&url)
        .set("User-Agent", APP_USER_AGENT)
        .set("Authorization", &format!("token {token}"))
        .send_json(ureq::json!(&args)).with_context(|| format!("Error creating repo {}", repo_path.display()))?;

    let created_repo: CreatedRepo = response.into_json()?;

    match repo.remote("origin", &created_repo.ssh_url) {
        Ok(_) => {},
        Err(e) if e.code() == ErrorCode::Exists => {},
        Err(e) => {
            return Err(e.into())
        }
    };

    Ok(())
}
