pub mod v1;
pub mod v2;

use crate::model::Tweet;
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Authentication {
    pub bearer_token: String,
}

#[async_trait]
pub trait TwitterClient {
    async fn get_id_for_username(&self, username: &str) -> anyhow::Result<u64>;

    async fn get_all_tweets_for_user(
        &self,
        user_id: u64,
        since_id: Option<u64>,
    ) -> anyhow::Result<Vec<Tweet>>;
}
