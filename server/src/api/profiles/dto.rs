use serde::{Deserialize, Serialize};

use crate::api::creation::{CreationCharacter, CreationWorld};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProfilesRequest {
    pub character: CreationCharacter,
    pub world: CreationWorld,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateProfilesData {
    pub world: String,
    pub character: String,
    pub key_story_beats: String,
}
