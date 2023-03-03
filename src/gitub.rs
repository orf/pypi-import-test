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

    let github_repo = match get_repo(&args.name, &token) {
        Ok(r) => r,
        Err(APIError::DoesNotExist) => {
            create_repo(&args, &token)?
        }
        Err(e) => {
            return Err(e.into());
        }
    };

    match repo.remote("origin", &github_repo.ssh_url) {
        Ok(_) => {}
        Err(e) if e.code() == ErrorCode::Exists => {}
        Err(e) => {
            return Err(e.into());
        }
    };

    Ok(())
}

pub fn get_repo(name: &String, token: &String) -> Result<CreatedRepo, APIError> {
    let org = "pypi-data";
    let url = format!("https://api.github.com/repos/{}/{}", org, name);
    let response = match ureq::get(&url)
        .set("User-Agent", APP_USER_AGENT)
        .set("Authorization", &format!("token {token}"))
        .call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404, _)) => {
            return Err(APIError::DoesNotExist);
        }
        Err(e) => return Err(APIError::Other(e.into())),
    };

    let repo: CreatedRepo = response.into_json().map_err(APIError::DecodeError)?;

    Ok(repo)
}

#[derive(Error, Debug)]
pub enum APIError {
    #[error("Repo already exists")]
    AlreadyExists,
    #[error("Repo does not exist")]
    DoesNotExist,
    #[error("Decode Error: {0}")]
    DecodeError(#[from] io::Error),
    #[error("Status: {0}: {1}")]
    Status(u16, String),
    #[error("Error: {0}")]
    Other(#[from] anyhow::Error),
}

pub fn create_repo(repo: &NewRepo, token: &String) -> Result<CreatedRepo, APIError> {
    let org = "pypi-data";
    let url = format!("https://api.github.com/orgs/{}/repos", org);
    match ureq::post(&url)
        .set("User-Agent", APP_USER_AGENT)
        .set("Authorization", &format!("token {token}"))
        .send_json(ureq::json!(&repo))
    {
        Ok(response) => {
            let created_repo: CreatedRepo =
                response.into_json().map_err(APIError::DecodeError)?;
            Ok(created_repo)
        }
        Err(ureq::Error::Status(422, _)) => Err(APIError::AlreadyExists),
        Err(ureq::Error::Status(status, response)) => {
            let retry_after = response.header("retry-after").map(|c| c.to_string());
            let reset = response.header("x-ratelimit-reset").map(|c| c.to_string());
            let response_text = response.into_string().unwrap();
            let resp = format!("Retry: {:?} Reset: {:?}, Body: {response_text}", retry_after, reset);
            Err(APIError::Status(status, resp))
        }
        Err(e) => Err(APIError::Other(e.into())),
    }
}
