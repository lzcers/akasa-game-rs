use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateProfilesRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProfilesData {
    pub world: String,
    pub protagonist: String,
    pub key_story_beats: String,
}
