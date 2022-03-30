mod model;
mod twitter;

use crate::model::DataFile;
use crate::twitter::v1::TwitterClientV1;
use crate::twitter::TwitterClient;
use anyhow::{bail, Context};
use clap::Parser;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    /// Rescan tweets that have already been loaded
    #[clap(long)]
    rescan: bool,
    /// Continue even if an account fails to download
    #[clap(long)]
    continue_on_error: bool,
    /// Use Twitter API 2 (Warning: Does not support Video and Gif downloads)
    #[clap(long)]
    use_api_v2: bool,
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

    let client: Arc<dyn TwitterClient> = if args.use_api_v2 {
        println!("Using Twitter API v2");
        Arc::new(TwitterClientV2::new(&auth)?)
    } else {
        println!("Using Twitter API v1.1");
        Arc::new(TwitterClientV1::new(&auth))
    };

    for account in usernames {
        download_account(
            &account,
            args.photos,
            args.videos,
            &args.out,
            args.rescan,
            &client,
        )
        .await?;
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
    _photos: bool,
    _videos: bool,
    out_dir: &Path,
    rescan: bool,
    twitter: &Arc<dyn TwitterClient>,
) -> anyhow::Result<()> {
    let user_id = twitter
        .get_id_for_username(username)
        .await
        .context("Unable to find user")?;
    let user_dir = get_directory(out_dir, username).await?;
    let mut data_file = DataFile::load(&user_dir, user_id)
        .await?
        .unwrap_or_else(|| DataFile::new(user_id));
    let since_id = if rescan {
        None
    } else {
        data_file.latest_tweet_id()
    };
    let new_tweets = twitter.get_all_tweets_for_user(user_id, since_id).await?;
    println!("Got {:?} new tweets for {}", new_tweets.len(), username);
    data_file.merge_tweets(new_tweets);
    data_file.save(&user_dir).await?;
    Ok(())
}

async fn get_directory(out_dir: &Path, username: &str) -> anyhow::Result<PathBuf> {
    let dir = out_dir.join(username);
    fs::create_dir_all(&dir)
        .await
        .context("Unable to create output directory")?;
    Ok(dir)
}
