use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerateCreationDraftTarget {
    Character,
    World,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationTraits {
    pub intellect: u8,
    pub physique: u8,
    pub endurance: u8,
    pub courage: u8,
    pub rationality: u8,
    pub altruism: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationCharacter {
    pub name: String,
    pub gender: String,
    pub age: u16,
    pub appearance: String,
    pub traits: CreationTraits,
    pub background: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationWorld {
    pub era: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCreationDraftRequest {
    pub target: GenerateCreationDraftTarget,
    pub character: CreationCharacter,
    pub world: CreationWorld,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateCreationDraftData {
    pub character: Option<CreationCharacter>,
    pub world: Option<CreationWorld>,
}
