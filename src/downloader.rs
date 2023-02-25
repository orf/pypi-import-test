use crate::create_urls::DownloadJob;
use futures_util::{stream, StreamExt, TryStreamExt};
use std::io::ErrorKind;
use tokio::{fs, io};

use std::path::PathBuf;

use reqwest::Client;
use tempdir::TempDir;

#[cfg(not(feature = "no_progress"))]
use crate::utils::create_pbar;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

async fn download(
    client: &Client,
    job: DownloadJob,
) -> anyhow::Result<(DownloadJob, TempDir, PathBuf)> {
    let download_dir = TempDir::new("download")?;
    let download_response = client.get(job.url.clone()).send().await?;
    let download_response = download_response.error_for_status()?;
    let save_path = download_dir.path().join("download");

    let file = fs::File::create(&save_path).await?;
    let mut writer = io::BufWriter::new(file);

    // From https://stackoverflow.com/questions/60964238/how-to-write-a-hyper-response-body-to-a-file
    let reader = download_response
        .bytes_stream()
        .map_err(|e| io::Error::new(ErrorKind::Other, e))
        .into_async_read();
    let body = to_tokio_async_read(reader);
    let mut reader = io::BufReader::new(body);
    io::copy(&mut reader, &mut writer).await?;
    Ok((job, download_dir, save_path))
}

fn to_tokio_async_read(r: impl futures::io::AsyncRead) -> impl tokio::io::AsyncRead {
    tokio_util::compat::FuturesAsyncReadCompatExt::compat(r)
}

pub fn download_multiple(
    packages: Vec<DownloadJob>,
) -> anyhow::Result<Vec<(DownloadJob, TempDir, PathBuf)>> {
    let total = packages.len();
    #[cfg(not(feature = "no_progress"))]
    let pbar = create_pbar(total as u64, "Downloading");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let results = runtime.block_on(async {
        let mut results = Vec::with_capacity(total);
        let client = Client::builder()
            .http2_prior_knowledge()
            .http2_adaptive_window(true)
            .user_agent(APP_USER_AGENT)
            .build()
            .unwrap();

        let mut result_stream = stream::iter(packages)
            .map(|job| {
                let client = &client;
                download(client, job)
            })
            .buffer_unordered(15);

        while let Some(res) = result_stream.next().await {
            #[cfg(not(feature = "no_progress"))]
            pbar.inc(1);
            match res {
                Ok(item) => {
                    results.push(item);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(results)
    })?;

    Ok(results)
}
