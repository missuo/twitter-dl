use crate::model::{Media, MediaType, Tweet};
use crate::{Authentication, TwitterClient};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use egg_mode::entities::MediaEntity;
use egg_mode::Token;

pub struct TwitterClientV1 {
    token: Token,
}

impl TwitterClientV1 {
    pub fn new(auth: &Authentication) -> Self {
        Self {
            token: Token::Bearer(auth.bearer_token.clone()),
        }
    }
}

#[async_trait]
impl TwitterClient for TwitterClientV1 {
    async fn get_id_for_username(&self, username: &str) -> anyhow::Result<u64> {
        let user = egg_mode::user::show(username.to_string(), &self.token)
            .await
            .context("Unable to find username")?;
        Ok(user.response.id)
    }

    async fn get_all_tweets_for_user(
        &self,
        user_id: u64,
        since_id: Option<u64>,
    ) -> anyhow::Result<Vec<Tweet>> {
        let mut timeline =
            egg_mode::tweet::user_timeline(user_id, true, false, &self.token).with_page_size(200);
        let mut tweets = Vec::new();
        loop {
            let (t2, mut new) = timeline
                .older(since_id)
                .await
                .context("Unable to fetch tweets")?;
            timeline = t2;
            if new.is_empty() {
                break;
            } else {
                tweets.append(&mut new);
            }
        }
        Ok(tweets
            .into_iter()
            .map(Tweet::try_from)
            .collect::<Result<_, _>>()?)
    }
}

impl TryFrom<egg_mode::tweet::Tweet> for Tweet {
    type Error = anyhow::Error;

    fn try_from(tweet: egg_mode::tweet::Tweet) -> anyhow::Result<Self> {
        let media = tweet
            .entities
            .media
            .unwrap_or_default()
            .into_iter()
            .map(Media::try_from)
            .collect::<Result<_, _>>()?;
        Ok(Tweet {
            id: tweet.id,
            text: tweet.text,
            media,
        })
    }
}

impl TryFrom<MediaEntity> for Media {
    type Error = anyhow::Error;

    fn try_from(entity: MediaEntity) -> anyhow::Result<Self> {
        Ok(match entity.media_type {
            egg_mode::entities::MediaType::Photo => {
                Media::new(entity.id, MediaType::Photo, Some(entity.media_url_https))
            }
            egg_mode::entities::MediaType::Video => {
                Media::new(entity.id, MediaType::Video, Some(get_video_url(&entity)?))
            }
            egg_mode::entities::MediaType::Gif => {
                Media::new(entity.id, MediaType::Gif, Some(get_video_url(&entity)?))
            }
        })
    }
}

fn get_video_url(entity: &MediaEntity) -> anyhow::Result<String> {
    let info = entity
        .video_info
        .as_ref()
        .ok_or_else(|| anyhow!("Missing video info"))?;
    let best_variant = info
        .variants
        .iter()
        .filter(|v| v.bitrate.is_some())
        .max_by_key(|v| v.bitrate.unwrap())
        .ok_or_else(|| anyhow!("Missing video variant"))?;
    Ok(best_variant.url.clone())
}
