mod download;
mod model;
mod twitter;

use crate::download::{BulkDownloader, DownloadError};
use crate::model::{DataFile, MediaType, MODEL_VERSION};
use crate::twitter::v1::TwitterClientV1;
use crate::twitter::TwitterClient;
use anyhow::{bail, Context};
use clap::Parser;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use twitter::v2::TwitterClientV2;
use twitter::Authentication;

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    /// Path to the authentication details file
    #[clap(short, long, default_value = "./auth.json")]
    auth: PathBuf,
    /// Where to save downloaded media (a sub folder will be created for each username)
    #[clap(short, long, default_value = "./")]
    out: PathBuf,
    /// Username(s) to download from (comma seperated)
    #[clap(short, long)]
    users: Option<String>,
    /// File containing list of usernames to download from (one per line)
    #[clap(short, long)]
    list: Option<PathBuf>,
    /// Download photos
    #[clap(long)]
    photos: bool,
    /// Download videos
    #[clap(long)]
    videos: bool,
    /// Download gifs
    #[clap(long)]
    gifs: bool,
    /// Rescan tweets that have already been loaded
    #[clap(long)]
    rescan: bool,
    /// Continue even if an account fails to download
    #[clap(long)]
    continue_on_error: bool,
    /// Use Twitter API 2 (Warning: Does not support Video and Gif downloads)
    #[clap(long)]
    api_v2: bool,
    /// Number of downloads to do concurrently
    #[clap(long, default_value_t = 4)]
    concurrency: usize,
}

#[tokio::main]
async fn main() {
    if let Err(e) = main2().await {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
}

async fn main2() -> anyhow::Result<()> {
    let args: Args = Args::parse();
    if !args.out.is_dir() {
        bail!("Destination must be a directory");
    }
    let auth = fs::read_to_string(&args.auth)
        .await
        .context("Unable to read auth file")?;
    let auth =
        serde_json::from_str::<Authentication>(&auth).context("Unable to deserialize auth file")?;
    let usernames = parse_usernames(&args).await?;

    let client: Arc<dyn TwitterClient> = if args.api_v2 {
        println!("Using Twitter API v2");
        Arc::new(TwitterClientV2::new(&auth)?)
    } else {
        println!("Using Twitter API v1.1");
        Arc::new(TwitterClientV1::new(&auth))
    };

    let mut media_types = Vec::new();
    if args.photos {
        media_types.push(MediaType::Photo);
    }
    if args.videos {
        media_types.push(MediaType::Video);
    }
    if args.gifs {
        media_types.push(MediaType::Gif)
    }

    for account in usernames {
        if let Err(e) = download_account(
            &account,
            args.concurrency,
            &media_types,
            &args.out,
            args.rescan,
            &client,
        )
        .await
        {
            if args.continue_on_error {
                eprintln!("Error downloading tweets for: {}, ignoring...", account);
            } else {
                return Err(e);
            }
        }
    }
    Ok(())
}

async fn parse_usernames(args: &Args) -> anyhow::Result<Vec<String>> {
    let mut account_names = BTreeSet::new();
    if let Some(users) = &args.users {
        users.split(',').for_each(|s| {
            account_names.insert(s.to_string());
        });
    }
    if let Some(list) = &args.list {
        let list = fs::read_to_string(list)
            .await
            .context("Unable to read users list")?;
        list.lines().for_each(|l| {
            account_names.insert(l.to_string());
        });
    }
    if account_names.is_empty() {
        bail!("No usernames provided")
    }
    Ok(account_names.into_iter().collect())
}

async fn download_account(
    username: &str,
    concurrency: usize,
    media_types: &[MediaType],
    out_dir: &Path,
    rescan: bool,
    twitter: &Arc<dyn TwitterClient>,
) -> anyhow::Result<()> {
    let user_id = twitter
        .get_id_for_username(username)
        .await
        .context("Unable to find user")?;
    let user_dir = out_dir.join(username);
    fs::create_dir_all(&user_dir)
        .await
        .context("Unable to create output directory")?;
    let mut data_file = DataFile::load(&user_dir, user_id)
        .await?
        .unwrap_or_else(|| DataFile::new(user_id));
    let since_id = if rescan || data_file.version < MODEL_VERSION {
        println!("Refreshing all available tweets for {}", username);
        None
    } else {
        data_file.latest_tweet_id()
    };
    let new_tweets = twitter.get_all_tweets_for_user(user_id, since_id).await?;
    let new = data_file.merge_tweets(new_tweets);
    println!("Got {:?} new tweets for {}", new, username);
    data_file.save(&user_dir).await?;

    let mut downloader = BulkDownloader::new(concurrency, Duration::from_secs(3));

    for (tweet_index, tweet) in data_file.tweets.iter().enumerate() {
        for (media_index, media) in tweet.media.iter().enumerate() {
            if let Some((url, filename)) = media.is_download_candidate(tweet, media_types) {
                downloader.push_task(
                    url,
                    user_dir.join(&filename),
                    DownloadContext {
                        tweet_index,
                        media_index,
                        filename,
                    },
                );
            }
        }
    }

    let (handle, mut rx) = downloader.run();

    let mut counter = 0;
    if let Err(e) = async {
        while let Some((ctx, result)) = rx.recv().await {
            match result {
                Ok(_completed) => {
                    data_file.tweets[ctx.tweet_index].media[ctx.media_index].file_name =
                        Some(ctx.filename);
                    data_file.save(&user_dir).await.ok();
                    counter += 1;
                }
                Err(e) => match e {
                    DownloadError::DestinationExists(e) => {
                        eprintln!("File: {} already exists, skipping", e.display());
                    }
                    DownloadError::BadResponse(c, url) if c == 404 => {
                        eprintln!("File no longer available (404): {}, skipping", url);
                    }
                    _ => return Err(e.into()),
                },
            }
        }
        Ok(())
    }
    .await
    {
        handle.abort();
        return Err(e);
    }
    data_file
        .save(&user_dir)
        .await
        .context("Error saving data file")?;
    println!("Downloaded {} new files for {}", counter, username);
    Ok(())
}

struct DownloadContext {
    pub tweet_index: usize,
    pub media_index: usize,
    pub filename: String,
}
