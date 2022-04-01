use crate::download::download_task::{DownloadError, DownloadTask};
use crate::model::{DataFile, MediaType, MODEL_VERSION};
use crate::twitter::v1::TwitterClientV1;
use crate::twitter::v2::TwitterClientV2;
use crate::twitter::Authentication;
use crate::twitter::TwitterClient;
use crate::{DownloadArgs, FileExistsPolicy};
use anyhow::{bail, Context};
use futures::{stream, StreamExt};
use reqwest::Client;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;
use tokio::fs;

mod download_task;

pub async fn download(args: DownloadArgs) -> anyhow::Result<()> {
    if !args.out.is_dir() {
        bail!("Destination must be a directory");
    }
    let auth = fs::read_to_string(&args.auth)
        .await
        .context("Unable to read auth file")?;
    let auth =
        serde_json::from_str::<Authentication>(&auth).context("Unable to deserialize auth file")?;
    let usernames = parse_usernames(&args).await?;

    let client: Box<dyn TwitterClient> = if args.api_v2 {
        log::info!("Using Twitter API v2");
        Box::new(TwitterClientV2::new(&auth)?)
    } else {
        log::info!("Using Twitter API v1.1");
        Box::new(TwitterClientV1::new(&auth))
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

    let connection_pool = Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    for account in usernames {
        if let Err(e) = download_account(
            &account,
            args.concurrency,
            &media_types,
            &args.out,
            args.rescan,
            client.as_ref(),
            &args.file_exists_policy,
            &connection_pool,
        )
        .await
        {
            if args.continue_on_error {
                log::warn!("Error downloading tweets for: {}, ignoring...", account);
            } else {
                return Err(e);
            }
        }
    }
    Ok(())
}

async fn parse_usernames(args: &DownloadArgs) -> anyhow::Result<Vec<String>> {
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

#[allow(clippy::too_many_arguments)]
async fn download_account(
    username: &str,
    concurrency: usize,
    media_types: &[MediaType],
    out_dir: &Path,
    rescan: bool,
    twitter: &'_ dyn TwitterClient,
    file_exists_policy: &FileExistsPolicy,
    connection_pool: &Client,
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
        log::info!("Refreshing all available tweets for {}", username);
        None
    } else {
        data_file.latest_tweet_id()
    };
    let new_tweets = twitter.get_all_tweets_for_user(user_id, since_id).await?;
    let new = data_file.merge_tweets(new_tweets);
    log::info!("Got {:?} new tweets for {}", new, username);
    data_file.save(&user_dir).await?;

    let mut downloads = vec![];

    for (tweet_index, tweet) in data_file.tweets.iter().enumerate() {
        for (media_index, media) in tweet.media.iter().enumerate() {
            if let Some((url, filename)) = media.is_download_candidate(tweet, media_types) {
                downloads.push(DownloadTask {
                    client: connection_pool.clone(),
                    url,
                    destination: user_dir.join(&filename),
                    context: DownloadContext {
                        tweet_index,
                        media_index,
                        filename,
                    },
                    overwrite: file_exists_policy == &FileExistsPolicy::Overwrite,
                });
            }
        }
    }

    let mut counter = 0;
    let mut buffered = stream::iter(downloads)
        .map(DownloadTask::download)
        .buffer_unordered(concurrency);

    while let Some((result, ctx)) = buffered.next().await {
        match result {
            Ok(_completed) => {
                data_file.tweets[ctx.tweet_index].media[ctx.media_index].file_name =
                    Some(ctx.filename);
                data_file.save(&user_dir).await.ok();
                counter += 1;
            }
            Err(e) => match e {
                DownloadError::DestinationExists(..)
                    if file_exists_policy == &FileExistsPolicy::Adopt =>
                {
                    data_file.tweets[ctx.tweet_index].media[ctx.media_index].file_name =
                        Some(ctx.filename);
                    data_file.save(&user_dir).await.ok();
                }
                DownloadError::DestinationExists(e)
                    if file_exists_policy == &FileExistsPolicy::Warn =>
                {
                    log::warn!("File: {} already exists, skipping", e.display());
                }
                DownloadError::BadResponse(c, url) if c == 404 => {
                    // If there is a 404, not much we can do, presumably the tweet has
                    // been deleted at some point after we retrieved it
                    log::warn!("File no longer available (404): {}, skipping", url);
                }
                _ => return Err(e.into()),
            },
        }
    }

    data_file
        .save(&user_dir)
        .await
        .context("Error saving data file")?;
    log::info!("Downloaded {} files for {}", counter, username);

    Ok(())
}

struct DownloadContext {
    pub tweet_index: usize,
    pub media_index: usize,
    pub filename: String,
}
