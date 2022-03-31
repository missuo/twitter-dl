//! There doesn't yet seem to be a good Rust client that uses API V2

use crate::model::{Media, MediaType, Tweet};
use crate::twitter::{Authentication, TwitterClient};
use anyhow::{bail, Context};
use async_trait::async_trait;
use chrono::DateTime;
use maplit::hashmap;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{Client, Response, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;

const TIMEOUT_SEC: u64 = 10;

#[derive(Clone)]
pub struct TwitterClientV2 {
    client: Client,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TwitterResponse<T> {
    Ok(T),
    // Detect the case where the API returns 200, but contains errors
    #[allow(unused)]
    Error {
        errors: serde_json::Value,
    },
}

#[derive(Deserialize)]
struct ByUsernameResponse {
    data: ByUsernameData,
}

#[derive(Deserialize)]
struct ByUsernameData {
    id: String,
}

#[derive(Deserialize)]
struct GetTweetsResponse {
    #[serde(default)]
    data: Vec<GetTweetsTweet>,
    includes: Option<GetTweetsIncludes>,
    meta: GetTweetsMeta,
}

#[derive(Deserialize)]
pub struct GetTweetsTweet {
    id: String,
    text: String,
    created_at: String,
    #[serde(default)]
    attachments: GetTweetsTweetAttachment,
}

#[derive(Deserialize, Default)]
pub struct GetTweetsTweetAttachment {
    #[serde(default)]
    media_keys: Vec<String>,
}

#[derive(Deserialize, Default)]
struct GetTweetsIncludes {
    #[serde(default)]
    media: Vec<GetTweetsMedia>,
}

#[derive(Deserialize)]
struct GetTweetsMedia {
    media_key: String,
    #[serde(flatten)]
    variant: GetTweetsMediaVariant,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum GetTweetsMediaVariant {
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "photo")]
    Photo { url: String },
    #[serde(rename = "animated_gif")]
    Gif,
}

#[derive(Deserialize)]
struct GetTweetsMeta {
    next_token: Option<String>,
}

async fn deserialize_response<T: DeserializeOwned>(response: Response) -> anyhow::Result<T> {
    let status = response.status();
    let text = response.text().await.context("Bad response text")?;
    if !status.is_success() {
        let code = status.as_u16();
        bail!(format!("Response was not successful: {code}\n{text}"))
    }
    let twitter = match serde_json::from_str::<TwitterResponse<T>>(&text) {
        Ok(ok) => ok,
        Err(e) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(pretty) => {
                let pretty = serde_json::to_string_pretty(&pretty).unwrap();
                bail!(format!(
                    "Unable to deserialize due to: {e}\nContents:\n{pretty}"
                ))
            }
            Err(_) => bail!("Invalid JSON"),
        },
    };
    Ok(match twitter {
        TwitterResponse::Ok(ok) => ok,
        TwitterResponse::Error { .. } => bail!(text),
    })
}

impl TwitterClientV2 {
    pub fn new(auth: &Authentication) -> anyhow::Result<Self> {
        let mut headers = HeaderMap::new();
        let value = format!("Bearer {}", auth.bearer_token);
        let value = HeaderValue::from_str(&value)?;
        headers.insert(AUTHORIZATION, value);
        Ok(Self {
            client: Client::builder()
                .default_headers(headers)
                .timeout(Duration::from_secs(TIMEOUT_SEC))
                .build()?,
        })
    }

    // https://developer.twitter.com/en/docs/twitter-api/tweets/timelines/api-reference/get-users-id-tweets
    async fn get_tweets_for_user(
        &self,
        user_id: u64,
        since_id: Option<u64>,
        pagination_token: Option<String>,
    ) -> anyhow::Result<(Vec<Tweet>, Option<String>)> {
        let url =
            Url::from_str(&format!("https://api.twitter.com/2/users/{user_id}/tweets")).unwrap();
        let mut query = hashmap! {
            "exclude" => "retweets".to_string(),
            "max_results" => "100".to_string(),
            // Including `preview_image_url` ensures we do at least get video Ids
            "media.fields" => "url,type,media_key,preview_image_url".to_string(),
            "tweet.fields" => "created_at".to_string(),
            "expansions" => "attachments.media_keys".to_string(),
        };
        if let Some(since_id) = since_id {
            query.insert("since_id", since_id.to_string());
        }
        if let Some(pagination_token) = pagination_token {
            query.insert("pagination_token", pagination_token);
        }
        let response = self.client.get(url).query(&query).send().await?;
        let response = deserialize_response::<GetTweetsResponse>(response).await?;
        let media = response
            .includes
            .map(|i| i.media)
            .unwrap_or_else(Default::default);
        let tweets = convert_tweets(response.data, media)?;
        Ok((tweets, response.meta.next_token))
    }
}

#[async_trait]
impl TwitterClient for TwitterClientV2 {
    async fn get_id_for_username(&self, username: &str) -> anyhow::Result<u64> {
        let url = Url::from_str("https://api.twitter.com/2/users/by/username/").unwrap();
        let url = url.join(username).unwrap();
        let response = self.client.get(url).send().await?;
        let response = deserialize_response::<ByUsernameResponse>(response).await?;
        Ok(response.data.id.parse().context("Couldn't parse user id")?)
    }

    async fn get_all_tweets_for_user(
        &self,
        user_id: u64,
        since_id: Option<u64>,
    ) -> anyhow::Result<Vec<Tweet>> {
        let mut next_token = None;
        let mut results = Vec::new();
        loop {
            let (mut page, next) = self
                .get_tweets_for_user(user_id, since_id, next_token.clone())
                .await?;
            results.append(&mut page);
            if next.is_none() {
                break;
            } else {
                next_token = next;
            }
        }
        Ok(results)
    }
}

fn convert_tweets(
    tweets: Vec<GetTweetsTweet>,
    media: Vec<GetTweetsMedia>,
) -> anyhow::Result<Vec<Tweet>> {
    tweets
        .into_iter()
        .map(|tweet| {
            Ok(Tweet {
                id: u64::from_str(&tweet.id)?,
                timestamp: DateTime::parse_from_rfc3339(&tweet.created_at)?.timestamp(),
                text: tweet.text,
                media: tweet
                    .attachments
                    .media_keys
                    .into_iter()
                    .map(|key| {
                        let m = media
                            .iter()
                            .find(|m| m.media_key == key)
                            .context("Missing media item")?;
                        m.convert()
                    })
                    .collect::<anyhow::Result<_>>()?,
            })
        })
        .collect::<anyhow::Result<_>>()
}

impl GetTweetsMedia {
    fn convert(&self) -> anyhow::Result<Media> {
        // There doesn't seem to be a way to get the Video URLs at the moment :(
        // https://stackoverflow.com/questions/66211050/twitter-api-v2-video-url
        let (url, r#type) = match &self.variant {
            GetTweetsMediaVariant::Video => (None, MediaType::Video),
            GetTweetsMediaVariant::Photo { url } => (Some(url.to_string()), MediaType::Photo),
            GetTweetsMediaVariant::Gif => (None, MediaType::Gif),
        };
        let pos = self
            .media_key
            .find('_')
            .context("Unexpected media key format")?;
        let id = &self.media_key[pos + 1..]
            .parse()
            .context("Unable to parse media key")?;
        let url = url
            .map(|url| Url::from_str(&url))
            .map_or(Ok(None), |url| url.map(Some))?;
        Ok(Media::new(*id, r#type, url))
    }
}
