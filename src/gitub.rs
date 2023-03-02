use crate::job::APP_USER_AGENT;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize, Deserialize)]
struct NewRepo {
    name: String,
    description: Option<String>,
    private: bool,
}

pub fn create_repository(name: String) -> anyhow::Result<()> {
    // Get the GitHub token from the environment variable
    let token = env::var("IMPORT_GITHUB_TOKEN")?;

    // Set up the API request
    let org = "pypi-data";
    let url = format!("https://api.github.com/orgs/{}/repos", org);
    let repo = NewRepo {
        name,
        description: Some("This is a new repository".into()),
        private: true,
    };
    // Send the API request to create the new repository
    let response = ureq::post(&url)
        .set("User-Agent", APP_USER_AGENT)
        .set("Authorization", &format!("token {token}"))
        .send_json(ureq::json!(&repo))?;

    println!("Response: {}", response.into_string()?);

    Ok(())
}
