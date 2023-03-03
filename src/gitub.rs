use crate::combine::JsonIndex;
use crate::job::APP_USER_AGENT;
use git2::{ErrorCode, Repository};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{env, fs, io};
use thiserror::Error;

#[derive(Debug, Serialize)]
pub struct NewRepo {
    name: String,
    description: String,
    private: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreatedRepo {
    ssh_url: String,
}

pub fn create_repository(repo_path: PathBuf) -> anyhow::Result<()> {
    let repo = Repository::open(&repo_path)?;
    repo.set_head("refs/heads/main")?;
    repo.checkout_head(None)?;

    let index_json = fs::read_to_string(repo_path.join("index.json"))?;
    let index_json: JsonIndex = serde_json::from_str(&index_json)?;
    let repo_name = index_json.url.path().split('/').last().unwrap();

    // Get the GitHub token from the environment variable
    let token = env::var("IMPORT_GITHUB_TOKEN")?;

    // Set up the API request

    let args = NewRepo {
        name: repo_name.to_string(),
        description: format!(
            "PyPi code from {} to {}",
            index_json.earliest_release.date_naive(),
            index_json.latest_release.date_naive()
        ),
        private: false,
    };

    let created_repo = match create_repo(&args, &token) {
        Ok(r) => r,
        Err(CreateRepoError::AlreadyExists) => {
            delete_repo(&args.name, &token)?;
            create_repo(&args, &token)?
        }
        Err(e) => return Err(e.into()),
    };

    match repo.remote("origin", &created_repo.ssh_url) {
        Ok(_) => {}
        Err(e) if e.code() == ErrorCode::Exists => {}
        Err(e) => {
            return Err(e.into());
        }
    };

    Ok(())
}

pub fn delete_repo(name: &String, token: &String) -> anyhow::Result<()> {
    let org = "pypi-data";
    let url = format!("https://api.github.com/repos/{}/{}", org, name);
    ureq::delete(&url)
        .set("User-Agent", APP_USER_AGENT)
        .set("Authorization", &format!("token {token}"))
        .call()?;

    Ok(())
}

#[derive(Error, Debug)]
pub enum CreateRepoError {
    #[error("Repo already exists")]
    AlreadyExists,
    #[error("Decode Error: {0}")]
    DecodeError(#[from] io::Error),
    #[error("Status: {0}: {1}")]
    Status(u16, String),
    #[error("Error: {0}")]
    Other(#[from] anyhow::Error),
}

pub fn create_repo(repo: &NewRepo, token: &String) -> Result<CreatedRepo, CreateRepoError> {
    let org = "pypi-data";
    let url = format!("https://api.github.com/orgs/{}/repos", org);
    match ureq::post(&url)
        .set("User-Agent", APP_USER_AGENT)
        .set("Authorization", &format!("token {token}"))
        .send_json(ureq::json!(&repo))
    {
        Ok(response) => {
            let created_repo: CreatedRepo =
                response.into_json().map_err(CreateRepoError::DecodeError)?;
            Ok(created_repo)
        }
        Err(ureq::Error::Status(422, response)) => Err(CreateRepoError::AlreadyExists),
        Err(ureq::Error::Status(status, response)) => {
            Err(CreateRepoError::Status(status, response.into_string().unwrap()))
        },
        Err(e) => Err(CreateRepoError::Other(e.into())),
    }
}
