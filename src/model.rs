use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::btree_map::BTreeMap;
use std::path::Path;
use tokio::fs;
use url::Url;

// If we introduce new features to the model we will want to refresh as much of the data
// as possible. Although there is no guarantee we can be successful (because of the 3200)
// tweet limit in the API, and that tweets may have since been deleted, so model changes must
// always be backwards compatible with previous data.
pub const MODEL_VERSION: u64 = 1;

#[derive(Deserialize, Serialize, Debug)]
pub struct Tweet {
    pub id: u64,
    pub timestamp: i64,
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
    pub id: u64,
    pub r#type: MediaType,
    pub file_name: Option<String>,
    pub url: Option<Url>,
}

impl Media {
    pub fn new(id: u64, r#type: MediaType, url: Option<Url>) -> Self {
        Self {
            id,
            r#type,
            file_name: None,
            url,
        }
    }

    // If true then return the URL to download, and filename to save as
    pub fn is_download_candidate(
        &self,
        tweet: &Tweet,
        media_types: &[MediaType],
    ) -> Option<(Url, String)> {
        if !media_types.contains(&self.r#type) {
            return None;
        }
        // Only download if we haven't already got it
        if self.file_name.is_some() {
            return None;
        }
        // Only download if a URL is available
        self.url.as_ref().map(|url| {
            let dot_idx = url.path().rfind('.');
            let ext = dot_idx
                .map(|idx| url.path()[idx + 1..].to_string())
                .unwrap_or_else(String::new);
            let file_name = format!("{}_{}.{ext}", tweet.id, self.id);
            (url.clone(), file_name)
        })
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Video,
    Photo,
    Gif,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DataFile {
    pub user_id: u64,
    pub tweets: Vec<Tweet>,
    pub version: u64,
}

impl DataFile {
    pub fn new(user_id: u64) -> Self {
        Self {
            user_id,
            tweets: vec![],
            version: MODEL_VERSION,
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

    /// Returns number of not seen before tweets
    pub fn merge_tweets(&mut self, new_tweets: Vec<Tweet>) -> usize {
        let mut new = 0;
        let existing = std::mem::take(&mut self.tweets);
        let mut map = existing
            .into_iter()
            .map(|t| (t.id, t))
            .collect::<BTreeMap<_, _>>();
        for mut tweet in new_tweets {
            // We don't want to overwrite the filenames though
            if let Some(existing) = map.get(&tweet.id) {
                for media in &mut tweet.media {
                    if let Some(equal) = existing.media.iter().find(|m| m.id == media.id) {
                        media.file_name = equal.file_name.clone();
                    }
                }
            }
            if map.insert(tweet.id, tweet).is_none() {
                new += 1;
            }
        }
        self.tweets = map.into_values().collect();
        self.version = MODEL_VERSION;
        new
    }

    pub fn latest_tweet_id(&self) -> Option<u64> {
        self.tweets.last().map(|l| l.id)
    }
}
