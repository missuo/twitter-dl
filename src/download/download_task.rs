use reqwest::Client;
use std::fmt::Debug;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use url::Url;

pub struct CompletedDownload {
    /// The destination the file was saved at
    pub saved_at: PathBuf,
    /// Number of bytes written
    pub written: usize,
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("Destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("Error whilst writing to file: {0}")]
    FileError(#[source] std::io::Error),
    #[error("Destination is not a valid file path: {0}")]
    InvalidDestination(PathBuf),
    #[error("Error performing HTTP request: {0}")]
    RequestError(
        #[source]
        #[from]
        reqwest::Error,
    ),
    #[error("Received unsuccessful response code: {0}")]
    BadResponse(u16, Url),
}

pub struct DownloadTask<C> {
    /// HTTP connection pool
    pub client: Client,
    /// Url to download
    pub url: Url,
    /// Path to save the file at
    pub destination: PathBuf,
    /// Arbitrary data to pass through
    pub context: C,
    /// Whether to overwrite an existing file (will return error otherwise)
    pub overwrite: bool,
}

impl<C> DownloadTask<C> {
    pub async fn download(self) -> (Result<CompletedDownload, DownloadError>, C) {
        let result = download_impl(self.destination, self.url, self.client, self.overwrite).await;
        (result, self.context)
    }
}

async fn download_impl(
    destination: PathBuf,
    url: Url,
    client: Client,
    overwrite: bool,
) -> Result<CompletedDownload, DownloadError> {
    let parent = destination
        .parent()
        .ok_or_else(|| DownloadError::InvalidDestination(destination.clone()))?;
    let temp = NamedTempFile::new_in(parent).map_err(DownloadError::FileError)?;
    let mut file = File::from_std(temp.reopen().map_err(DownloadError::FileError)?);
    let mut request = client.get(url.clone()).send().await?;
    if !request.status().is_success() {
        return Err(DownloadError::BadResponse(request.status().as_u16(), url));
    }
    let mut written = 0;
    while let Some(chunk) = request.chunk().await? {
        written += chunk.len();
        file.write(chunk.as_ref())
            .await
            .map_err(DownloadError::FileError)?;
    }
    file.flush().await.map_err(DownloadError::FileError)?;
    if !overwrite && destination.exists() {
        return Err(DownloadError::DestinationExists(destination));
    }
    temp.persist(&destination)
        .map_err(|e| DownloadError::FileError(e.error))?;
    Ok(CompletedDownload {
        saved_at: destination,
        written,
    })
}
