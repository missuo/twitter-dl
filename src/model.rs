use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::btree_map::BTreeMap;
use std::path::Path;
use tokio::fs;

#[derive(Deserialize, Serialize, Debug)]
pub struct Tweet {
    pub id: u64,
    pub text: String,
    pub media: Vec<Media>,
}

impl PartialEq<Self> for Tweet {
    fn eq(&self, other: &Self) -> bool {
        self.id.eq(&other.id)
    }
}

impl Eq for Tweet {}

impl PartialOrd<Self> for Tweet {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.id.cmp(&other.id))
    }
}

impl Ord for Tweet {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Media {
    pub r#type: MediaType,
    pub file_name: Option<String>,
    pub url: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub enum MediaType {
    Video,
    Photo,
    Gif,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DataFile {
    pub user_id: u64,
    pub tweets: Vec<Tweet>,
}

impl DataFile {
    pub fn new(user_id: u64) -> Self {
        Self {
            user_id,
            tweets: vec![],
        }
    }

    pub async fn load(user_dir: &Path, validate_user_id: u64) -> anyhow::Result<Option<DataFile>> {
        let data_file = user_dir.join("tweets.json");
        Ok(if data_file.exists() {
            let data_file = fs::read_to_string(&data_file)
                .await
                .context("Unable to read data file")?;
            let mut data_file = serde_json::from_str::<Self>(&data_file)
                .context("Unable to deserialize data file")?;
            if data_file.user_id != validate_user_id {
                bail!("User id mismatch! The username you have provided is not for the same account that was previously downloaded")
            }
            data_file.tweets.sort();
            Some(data_file)
        } else {
            None
        })
    }

    pub async fn save(&self, user_dir: &Path) -> anyhow::Result<()> {
        let text = serde_json::to_string_pretty(&self).unwrap();
        fs::write(user_dir.join("tweets.json"), &text)
            .await
            .context("Unable to write data file")
    }

    pub fn merge_tweets(&mut self, new_tweets: Vec<Tweet>) {
        let existing = std::mem::take(&mut self.tweets);
        let mut map = existing
            .into_iter()
            .map(|t| (t.id, t))
            .collect::<BTreeMap<_, _>>();
        for tweet in new_tweets {
            map.insert(tweet.id, tweet);
        }
        self.tweets = map.into_values().collect();
    }

    pub fn latest_tweet_id(&self) -> Option<u64> {
        self.tweets.last().map(|l| l.id)
    }
}
