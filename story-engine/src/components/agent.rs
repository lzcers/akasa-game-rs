use agent::{agent::Context, core::Message};
use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};

use crate::prompts::{
    character_prompt::CHARACTER_PROMPT,
    fate_weaver_prompt::{FATE_BASE_SYSTEM_PROMPT, OUTPUT_SCHEMA},
    upper_narrator_prompt::UPPER_NARRATOR_PROMPT,
};
#[derive(Component, Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub sys_prompt: String,
    pub role: AgentRole,
    pub output_type: AgentOutputType,
    pub context: Context,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Simulator;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applicator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentOutputType {
    #[serde(alias = "world_snapshot", alias = "character_options")]
    Json,
    #[serde(alias = "simulation_text", alias = "narration")]
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Simulator,
    Narrator,
    Character,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct PendingReasoning;

impl Agent {
    pub fn new_fate_weaver(
        world_profile: &str,
        character_profile: &str,
        key_story_beats: &str,
    ) -> Self {
        let system_prompt = FATE_BASE_SYSTEM_PROMPT
            .replace("{world_profile}", world_profile)
            .replace("{character_profile}", character_profile)
            .replace("{key_story_beats}", key_story_beats)
            .replace("{output_schema}", OUTPUT_SCHEMA);
        Self::new_with_role(
            AgentRole::Simulator,
            AgentOutputType::Json,
            "FateWeaver",
            system_prompt,
        )
    }

    pub fn new_upper_narrator(world_profile: &str, character_profile: &str) -> Self {
        let system_prompt = UPPER_NARRATOR_PROMPT
            .replace("{world_profile}", world_profile)
            .replace("{character_profile}", character_profile);
        Self::new_with_role(
            AgentRole::Narrator,
            AgentOutputType::Text,
            "UpperNarrator",
            system_prompt,
        )
    }

    pub fn new_character_agent(
        character_name: &str,
        world_profile: &str,
        character_profile: &str,
    ) -> Self {
        let system_prompt = CHARACTER_PROMPT
            .replace("{world_profile}", world_profile)
            .replace("{character_profile}", character_profile);
        Self::new_with_role(
            AgentRole::Character,
            AgentOutputType::Json,
            character_name,
            system_prompt,
        )
    }

    pub fn from_context(
        output_type: AgentOutputType,
        name: impl Into<String>,
        sys_prompt: impl Into<String>,
        context: Context,
    ) -> Self {
        Self::from_context_with_role(AgentRole::Simulator, output_type, name, sys_prompt, context)
    }

    pub fn from_context_with_role(
        role: AgentRole,
        output_type: AgentOutputType,
        name: impl Into<String>,
        sys_prompt: impl Into<String>,
        context: Context,
    ) -> Self {
        Self {
            role,
            output_type,
            name: name.into(),
            sys_prompt: sys_prompt.into(),
            context,
        }
    }

    pub fn new(
        output_type: AgentOutputType,
        name: impl Into<String>,
        system_prompt: String,
    ) -> Self {
        Self::new_with_role(AgentRole::Simulator, output_type, name, system_prompt)
    }

    pub fn new_with_role(
        role: AgentRole,
        output_type: AgentOutputType,
        name: impl Into<String>,
        system_prompt: String,
    ) -> Self {
        let mut context = Context::new();
        context.add_message(Message::system(&system_prompt));
        Self {
            role,
            output_type,
            name: name.into(),
            sys_prompt: system_prompt,
            context,
        }
    }

    pub fn append_user_message(&mut self, content: &str) -> Message {
        let message = Message::user(content);
        self.context.add_message(message.clone());
        message
    }

    pub fn append_assistant_message(&mut self, content: &str) -> Message {
        let message = Message::assistant(content);
        self.context.add_message(message.clone());
        message
    }

    pub fn revert(&mut self) -> bool {
        self.context.rollback_latest_input()
    }
}
