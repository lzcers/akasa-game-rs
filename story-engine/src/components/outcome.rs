use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PLAYER_CHARACTER_NAME: &str = "玩家角色";

// 叙事者输出的故事文本
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct NarrationOutcome {
    pub turn_id: u64,
    pub content: String,
}

// 模拟者输出的内容
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SimulationOutcome {
    pub turn_id: u64,
    pub content: String,
}

/// 角色决策状态：保存本轮已确认的一组角色行动，以及当前单玩家玩家角色模式的候选项。
#[derive(Component, Debug, Clone)]
pub struct CharacterDecisionState {
    committed_actions: Vec<PlayerActionItem>,
    choices: Vec<PendingCharacterChoice>,
}

impl CharacterDecisionState {
    pub fn from_archive(
        committed_actions: Vec<PlayerActionItem>,
        choices: Vec<PendingCharacterChoice>,
    ) -> Self {
        Self {
            committed_actions: committed_actions_or_start(committed_actions),
            choices,
        }
    }

    pub fn committed_actions(&self) -> &[PlayerActionItem] {
        &self.committed_actions
    }

    pub fn committed_action(&self) -> String {
        summarize_actions(&self.committed_actions)
    }

    pub fn choices(&self) -> &[PendingCharacterChoice] {
        &self.choices
    }

    pub fn replace_with_options(&mut self, options: CharacterOptions) {
        self.choices = options
            .options
            .into_iter()
            .enumerate()
            .map(|(index, option)| PendingCharacterChoice {
                id: format!("choice-{}", index + 1),
                option,
            })
            .collect();
    }

    pub fn commit_action(&mut self, action: &str) -> String {
        let action = action.trim().to_string();
        self.choices.clear();
        self.committed_actions = vec![PlayerActionItem::character_free_text(action.clone())];
        action
    }

    pub fn commit_actions(&mut self, actions: Vec<PlayerActionItem>) -> Vec<PlayerActionItem> {
        let actions = normalize_action_items(actions);
        self.choices.clear();
        self.committed_actions = actions.clone();
        actions
    }

    pub fn has_action(&self, action: &str) -> bool {
        self.choices
            .iter()
            .any(|choice| choice.option.action == action)
    }

    pub fn choice_for_action(&self, action: &str) -> Option<&CharacterOption> {
        self.choices
            .iter()
            .find(|choice| choice.option.action == action)
            .map(|choice| &choice.option)
    }
}

impl Default for CharacterDecisionState {
    fn default() -> Self {
        Self {
            committed_actions: vec![PlayerActionItem::character_free_text("start")],
            choices: Vec::new(),
        }
    }
}

/// 玩家角色 Agent 返回给玩家的一组选项。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct CharacterOptions {
    #[serde(default)]
    pub options: Vec<CharacterOption>,
}

/// 单个可供玩家选择的玩家角色行动。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CharacterOption {
    pub title: String,
    pub action: String,
    pub motivation_and_risk: String,
}

/// 玩家提交玩家角色行动时携带的输入类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlayerActionType {
    SelectedOption,
    FreeText,
}

/// 一名玩家操控一个故事角色提交的行动。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlayerActionItem {
    #[serde(alias = "characterName")]
    pub character_name: String,
    #[serde(alias = "playerId")]
    pub player_id: Option<String>,
    #[serde(alias = "actionType")]
    pub action_type: PlayerActionType,
    pub title: String,
    pub action: String,
    #[serde(alias = "motivationAndRisk")]
    pub motivation_and_risk: String,
}

impl Default for PlayerActionItem {
    fn default() -> Self {
        Self {
            character_name: DEFAULT_PLAYER_CHARACTER_NAME.to_string(),
            player_id: None,
            action_type: PlayerActionType::FreeText,
            title: String::new(),
            action: String::new(),
            motivation_and_risk: String::new(),
        }
    }
}

impl PlayerActionItem {
    pub fn character_selected_option(option: &CharacterOption) -> Self {
        Self {
            character_name: DEFAULT_PLAYER_CHARACTER_NAME.to_string(),
            player_id: None,
            action_type: PlayerActionType::SelectedOption,
            title: option.title.clone(),
            action: option.action.clone(),
            motivation_and_risk: option.motivation_and_risk.clone(),
        }
    }

    pub fn character_free_text(action: impl Into<String>) -> Self {
        let action = action.into();
        Self {
            character_name: DEFAULT_PLAYER_CHARACTER_NAME.to_string(),
            player_id: None,
            action_type: PlayerActionType::FreeText,
            title: String::new(),
            action,
            motivation_and_risk: String::new(),
        }
    }

    pub fn normalized(mut self) -> Self {
        self.character_name = normalize_character_name(&self.character_name);
        self.player_id = self
            .player_id
            .map(|player_id| player_id.trim().to_string())
            .filter(|player_id| !player_id.is_empty());
        self.title = self.title.trim().to_string();
        self.action = self.action.trim().to_string();
        self.motivation_and_risk = self.motivation_and_risk.trim().to_string();
        if self.title.is_empty() && self.action_type == PlayerActionType::SelectedOption {
            self.title = self.action.clone();
        }
        self
    }
}

/// 玩家提交到 ECS 的角色行动集合。单玩家模式是只有一项 character 行动的特例。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PlayerActionInput {
    pub actions: Vec<PlayerActionItem>,
}

impl PlayerActionInput {
    pub fn single(action_type: PlayerActionType, action: impl Into<String>) -> Self {
        let action = action.into();
        let item = match action_type {
            PlayerActionType::SelectedOption => PlayerActionItem {
                action_type,
                title: action.clone(),
                action,
                ..PlayerActionItem::default()
            },
            PlayerActionType::FreeText => PlayerActionItem::character_free_text(action),
        };
        Self {
            actions: vec![item],
        }
    }
}

/// 可供外部玩家提交的玩家角色候选项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingCharacterChoice {
    pub id: String,
    pub option: CharacterOption,
}

fn normalize_action_items(actions: Vec<PlayerActionItem>) -> Vec<PlayerActionItem> {
    let actions = actions
        .into_iter()
        .map(PlayerActionItem::normalized)
        .filter(|item| !item.action.is_empty())
        .collect::<Vec<_>>();
    committed_actions_or_start(actions)
}

fn committed_actions_or_start(actions: Vec<PlayerActionItem>) -> Vec<PlayerActionItem> {
    if actions.is_empty() {
        vec![PlayerActionItem::character_free_text("start")]
    } else {
        actions
    }
}

fn normalize_character_name(character_name: &str) -> String {
    let character_name = character_name.trim();
    if character_name.is_empty() {
        DEFAULT_PLAYER_CHARACTER_NAME.to_string()
    } else {
        character_name.to_string()
    }
}

fn summarize_actions(actions: &[PlayerActionItem]) -> String {
    match actions {
        [] => "start".to_string(),
        [single] => single.action.clone(),
        many => many
            .iter()
            .map(|item| format!("{}: {}", item.character_name, item.action))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}
