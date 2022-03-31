use futures::{stream, StreamExt};
use reqwest::Client;
use std::fmt::Debug;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use url::Url;

pub type DownloadRx<C> = mpsc::Receiver<(C, Result<CompletedDownload, DownloadError>)>;
pub type DownloadTx<C> = mpsc::Sender<(C, Result<CompletedDownload, DownloadError>)>;

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

struct DownloadTask<C> {
    /// HTTP connection pool
    client: Client,
    /// Url to download
    url: Url,
    /// Path to save the file at
    destination: PathBuf,
    /// Arbitrary data to pass through
    context: C,
    /// A channel to send the result to
    sender: DownloadTx<C>,
}

impl<C> DownloadTask<C> {
    async fn download(self) {
        let result = download_impl(self.destination, self.url, self.client).await;
        self.sender.send((self.context, result)).await.ok();
    }
}

async fn download_impl(
    destination: PathBuf,
    url: Url,
    client: Client,
) -> Result<CompletedDownload, DownloadError> {
    if destination.exists() {
        return Err(DownloadError::DestinationExists(destination));
    }
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
    temp.persist(&destination)
        .map_err(|e| DownloadError::FileError(e.error))?;
    Ok(CompletedDownload {
        saved_at: destination,
        written,
    })
}

pub struct BulkDownloader<C> {
    client: Client,
    tasks: Vec<DownloadTask<C>>,
    tx: DownloadTx<C>,
    rx: DownloadRx<C>,
    concurrency: usize,
}

impl<C: Send + 'static> BulkDownloader<C> {
    pub fn new(concurrency: usize, connect_timeout: Duration) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            client: Client::builder()
                .connect_timeout(connect_timeout)
                .build()
                .unwrap(),
            tasks: Default::default(),
            tx,
            rx,
            concurrency,
        }
    }

    pub fn push_task(&mut self, url: Url, destination: PathBuf, context: C) {
        self.tasks.push(DownloadTask {
            client: self.client.clone(),
            url,
            destination,
            context,
            sender: self.tx.clone(),
        })
    }

    pub fn run(self) -> (JoinHandle<()>, DownloadRx<C>) {
        let handle = tokio::spawn(async move {
            stream::iter(self.tasks)
                .map(DownloadTask::download)
                .buffer_unordered(self.concurrency)
                .collect::<Vec<_>>()
                .await;
        });
        (handle, self.rx)
    }
}
