use crate::model::{Media, MediaType, Tweet};
use anyhow::{anyhow, bail, Context};
use maplit::hashmap;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{Client, Response, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;

const TIMEOUT_SEC: u64 = 10;

#[derive(Clone)]
pub struct TwitterClient {
    client: Client,
}

#[derive(Deserialize)]
pub struct Authentication {
    bearer_token: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TwitterResponse<T> {
    Ok(T),
    // Detect the case where the API returns 200, but contains errors
    #[allow(unused)]
    Error { errors: serde_json::Value },
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
    data: Vec<GetTweetsTweet>,
    includes: GetTweetsIncludes,
    meta: GetTweetsMeta,
}

#[derive(Deserialize)]
pub struct GetTweetsTweet {
    id: String,
    text: String,
    #[serde(default)]
    attachments: GetTweetsTweetAttachment,
}

#[derive(Deserialize, Default)]
pub struct GetTweetsTweetAttachment {
    #[serde(default)]
    media_keys: Vec<String>,
}

#[derive(Deserialize)]
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
    Video { preview_image_url: String },
    #[serde(rename = "photo")]
    Photo { url: String },
    #[serde(rename = "animated_gif")]
    Gif { preview_image_url: String },
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
                bail!(format!("Unable to deserialize due to: {e}\nContents:\n{pretty}"))
            }
            Err(_) => bail!("Invalid JSON"),
        },
    };
    Ok(match twitter {
        TwitterResponse::Ok(ok) => ok,
        TwitterResponse::Error { .. } => bail!(format!("{text}")),
    })
}

impl TwitterClient {
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

    pub async fn get_id_for_username(&self, username: &str) -> anyhow::Result<String> {
        let url = Url::from_str("https://api.twitter.com/2/users/by/username/").unwrap();
        let url = url.join(username).unwrap();
        let response = self.client.get(url).send().await?;
        let response = deserialize_response::<ByUsernameResponse>(response).await?;
        Ok(response.data.id)
    }

    async fn get_tweets_for_user(
        &self,
        user_id: &str,
        since_id: Option<String>,
        pagination_token: Option<String>,
    ) -> anyhow::Result<(Vec<Tweet>, Option<String>)> {
        let url =
            Url::from_str(&format!("https://api.twitter.com/2/users/{user_id}/tweets")).unwrap();
        let mut query = hashmap! {
            "exclude" => "retweets".to_string(),
            "max_results" => "100".to_string(),
            "media.fields" => "url,type,media_key,preview_image_url".to_string(),
            "expansions" => "attachments.media_keys".to_string(),
        };
        if let Some(since_id) = since_id {
            query.insert("since_id", since_id);
        }
        if let Some(pagination_token) = pagination_token {
            query.insert("pagination_token", pagination_token);
        }
        let response = self.client.get(url).query(&query).send().await?;
        let response = deserialize_response::<GetTweetsResponse>(response).await?;
        let tweets = convert_tweets(response.data, response.includes.media)?;
        Ok((tweets, response.meta.next_token))
    }

    pub async fn get_all_tweets_for_user(
        &self,
        user_id: &str,
        since_id: Option<String>,
    ) -> anyhow::Result<Vec<Tweet>> {
        let mut next_token = None;
        let mut results = Vec::new();
        loop {
            let (mut page, next) = self
                .get_tweets_for_user(user_id, since_id.clone(), next_token.clone())
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
                text: tweet.text,
                media: tweet
                    .attachments
                    .media_keys
                    .into_iter()
                    .map(|key| {
                        let m = media
                            .iter()
                            .find(|m| m.media_key == key)
                            .ok_or_else(|| anyhow!("Missing media item"))?;
                        m.convert()
                    })
                    .collect::<anyhow::Result<_>>()?,
            })
        })
        .collect::<anyhow::Result<_>>()
}

impl GetTweetsMedia {
    fn convert(&self) -> anyhow::Result<Media> {
        let (url, r#type) = match &self.variant {
            GetTweetsMediaVariant::Video { preview_image_url } => {
                (preview_image_url.to_string(), MediaType::Video)
            }
            GetTweetsMediaVariant::Photo { url } => (url.to_string(), MediaType::Photo),
            GetTweetsMediaVariant::Gif { preview_image_url } => {
                (preview_image_url.to_string(), MediaType::Photo)
            }
        };
        Ok(Media {
            r#type,
            file_name: None,
            url,
        })
    }
}
