use agent::agent::Context as AgentContext;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use story_engine::components::outcome::PlayerActionItem;

use crate::database::AppDatabase;
use crate::session_history::{RoundHistoryEntry, TurnPhase};

mod codec;
mod entity_contexts;
mod rounds;
mod schema;
mod sessions;
mod story_edges;
mod story_path;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct SessionArchiveRepository {
    db: AppDatabase,
}

#[derive(Debug, Clone)]
pub struct StoredSessionMetadata {
    pub session_id: String,
    pub character_name: String,
    pub world_profile: String,
    pub character_profile: String,
    pub key_story_beats: String,
    pub phase: TurnPhase,
    pub turn_index: u64,
    pub active_turn_id: u64,
    pub flow_end: bool,
}

#[derive(Debug, Clone)]
pub struct StoredEntityContext {
    pub entity_name: String,
    pub context: AgentContext,
}

#[derive(Debug, Clone)]
pub struct StoredStoryEdgeAction {
    pub round: u64,
    pub action: PlayerActionItem,
}

#[derive(Debug, Clone)]
pub struct StoredChoiceExploration {
    pub round: u64,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct PreparedBacktrackBranch {
    pub source_round: u64,
    pub branch_round: u64,
    pub reused_existing_branch: bool,
    pub requires_generation: bool,
}

#[derive(Debug, Clone)]
pub struct StoredSessionRoundPage {
    pub rounds: Vec<RoundHistoryEntry>,
    pub next_before_round: Option<u64>,
    pub has_more: bool,
}
const ROOT_NODE_ID: &str = "start";
const DEFAULT_PLAYER_CHARACTER_NAME: &str = "玩家角色";

fn normalized_character_name(character_name: &str) -> String {
    let character_name = character_name.trim();
    if character_name.is_empty() {
        DEFAULT_PLAYER_CHARACTER_NAME.to_string()
    } else {
        character_name.to_string()
    }
}

fn normalize_action_character_name(
    action: PlayerActionItem,
    session_character_name: &str,
) -> PlayerActionItem {
    let session_character_name = normalized_character_name(session_character_name);
    if action.character_name.trim().is_empty()
        || action.character_name == DEFAULT_PLAYER_CHARACTER_NAME
    {
        PlayerActionItem {
            character_name: session_character_name,
            ..action
        }
    } else {
        action
    }
}

fn session_character_name(conn: &Connection, session_id: &str) -> Result<String> {
    let character_name = conn
        .query_row(
            r#"
            SELECT character_name
            FROM session_characters
            WHERE session_id = ?1
                AND is_playable = 1
            ORDER BY created_at ASC, character_name ASC
            LIMIT 1
            "#,
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("failed to load session character name")?;
    Ok(normalized_character_name(
        character_name.as_deref().unwrap_or_default(),
    ))
}
