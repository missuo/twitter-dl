use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Tweet {
    pub id: u64,
    pub text: String,
    pub media: Vec<Media>,
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
}

#[derive(Deserialize, Serialize, Debug)]
pub struct DataFile {
    pub user_id: String,
    pub tweets: Vec<Tweet>,
}
