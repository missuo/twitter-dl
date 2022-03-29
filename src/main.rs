mod model;
mod twitter;

use crate::twitter::{Authentication, TwitterClient};
use anyhow::Context;
use clap::Parser;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    /// Path to the authentication details file
    #[clap(short, long, default_value = "./auth.json")]
    auth: PathBuf,
    /// Where to save downloaded media (a sub folder will be created for each username)
    #[clap(short, long, default_value = "./")]
    destination: PathBuf,
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
}

#[tokio::main]
async fn main() {
    if let Err(e) = main2().await {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
}

async fn main2() -> anyhow::Result<()> {
    let args = Args::parse();
    let auth = fs::read_to_string(&args.auth)
        .await
        .context("Unable to read auth file")?;
    let auth =
        serde_json::from_str::<Authentication>(&auth).context("Unable to deserialize auth file")?;
    let usernames = parse_usernames(&args).await?;
    let client = TwitterClient::new(&auth)?;

    for account in usernames {
        download_account(
            account,
            args.photos,
            args.videos,
            &args.destination,
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
    Ok(account_names.into_iter().collect())
}

async fn download_account(
    username: String,
    _photos: bool,
    _videos: bool,
    _destination: &Path,
    twitter: &TwitterClient,
) -> anyhow::Result<()> {
    let id = twitter.get_id_for_username(&username).await.context("Unable to find user")?;
    let tweets = twitter.get_all_tweets_for_user(&id, None).await?;
    println!("{} {id}", username);
    println!("{:#?}", tweets.len());
    Ok(())
}
