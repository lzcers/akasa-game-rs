use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};

#[derive(Component, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TurnFlow {
    pub turn_index: u64,
    pub stage: TurnStage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStage {
    #[default]
    Start,
    Simulation,
    Application,
    AwaitingPlayer,
    TurnCompleted,
    Ended,
    Failed,
}

impl Default for TurnFlow {
    fn default() -> Self {
        Self {
            turn_index: 0,
            stage: TurnStage::Start,
        }
    }
}

impl TurnFlow {
    pub const fn active_turn_id(self) -> u64 {
        match self.stage {
            TurnStage::Start | TurnStage::TurnCompleted | TurnStage::Ended => self.turn_index,
            TurnStage::Simulation
            | TurnStage::Application
            | TurnStage::AwaitingPlayer
            | TurnStage::Failed => self.turn_index + 1,
        }
    }

    pub fn finish_turn(&mut self) {
        self.turn_index = self.active_turn_id();
        self.stage = TurnStage::TurnCompleted;
    }

    pub fn end(&mut self) {
        self.turn_index = self.active_turn_id();
        self.stage = TurnStage::Ended;
    }

    pub fn advance(&mut self) {
        match self.stage {
            TurnStage::Start | TurnStage::TurnCompleted => {
                self.stage = TurnStage::Simulation;
            }
            _ => {}
        }
    }
}

impl TurnStage {
    pub const fn is_stable(self) -> bool {
        matches!(
            self,
            TurnStage::Start
                | TurnStage::AwaitingPlayer
                | TurnStage::TurnCompleted
                | TurnStage::Ended
                | TurnStage::Failed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_and_finishes_turn_without_reusing_turn_ids() {
        let mut flow = TurnFlow::default();

        flow.advance();
        assert_eq!(flow.active_turn_id(), 1);
        assert_eq!(flow.stage, TurnStage::Simulation);

        flow.finish_turn();
        assert_eq!(flow.turn_index, 1);
        assert_eq!(flow.active_turn_id(), 1);
        assert_eq!(flow.stage, TurnStage::TurnCompleted);

        flow.advance();
        assert_eq!(flow.active_turn_id(), 2);
        assert_eq!(flow.stage, TurnStage::Simulation);
    }

    #[test]
    fn computes_active_turn_id_from_stage() {
        assert_eq!(
            TurnFlow {
                turn_index: 3,
                stage: TurnStage::Start,
            }
            .active_turn_id(),
            3
        );
        assert_eq!(
            TurnFlow {
                turn_index: 3,
                stage: TurnStage::Simulation,
            }
            .active_turn_id(),
            4
        );
        assert_eq!(
            TurnFlow {
                turn_index: 3,
                stage: TurnStage::AwaitingPlayer,
            }
            .active_turn_id(),
            4
        );
        assert_eq!(
            TurnFlow {
                turn_index: 4,
                stage: TurnStage::TurnCompleted,
            }
            .active_turn_id(),
            4
        );
    }

    #[test]
    fn accepts_only_current_stage_names() {
        assert_eq!(
            serde_json::from_str::<TurnStage>("\"awaiting_player\"").unwrap(),
            TurnStage::AwaitingPlayer
        );
        assert!(serde_json::from_str::<TurnStage>("\"simulation_ready\"").is_err());
        assert!(serde_json::from_str::<TurnStage>("\"awaiting_player_choice\"").is_err());
        assert!(serde_json::from_str::<TurnStage>("\"story_ended\"").is_err());
    }
}
