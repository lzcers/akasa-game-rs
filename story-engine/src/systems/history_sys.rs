use bevy_ecs::{
    change_detection::{DetectChanges, Mut, Ref},
    entity::Entity,
    hierarchy::ChildOf,
    system::Query,
};

use crate::{
    components::{
        outcome::NarrationOutcome,
        turn_flow::{TurnFlow, TurnStage},
    },
    resources::{
        history::SessionHistoryLog, protagonist_action::ProtagonistDecisionState,
        world_snapshot::WorldSnapshot,
    },
};

#[allow(clippy::type_complexity)]
pub fn history_sys(
    mut sessions: Query<(
        Entity,
        Ref<TurnFlow>,
        Ref<WorldSnapshot>,
        Ref<ProtagonistDecisionState>,
        Mut<SessionHistoryLog>,
    )>,
    narrations: Query<(&ChildOf, Ref<NarrationOutcome>)>,
) {
    for (session_entity, flow, world_snapshot, decision_state, mut history_log) in sessions
        .iter_mut()
        .filter(|(_, flow, ..)| flow.active_turn_id != 0)
    {
        let narration_changed = narrations
            .iter()
            .filter(|(owner, _)| owner.parent() == session_entity)
            .any(|(_, outcome)| outcome.is_changed());
        if !flow.is_changed()
            && !world_snapshot.is_changed()
            && !decision_state.is_changed()
            && !narration_changed
        {
            continue;
        }

        history_log.set_world_snapshot(flow.active_turn_id, world_snapshot.clone());

        if let Some((_, outcome)) = narrations.iter().find(|(owner, outcome)| {
            owner.parent() == session_entity && outcome.turn_id == flow.active_turn_id
        }) {
            history_log.set_narration(flow.active_turn_id, outcome.content.clone());
        }

        if !decision_state.choices().is_empty() {
            history_log.set_choices(flow.active_turn_id, decision_state.choices().to_vec());
        }

        if matches!(flow.stage, TurnStage::TurnCompleted | TurnStage::Ended) {
            history_log.set_committed_action(
                flow.active_turn_id,
                decision_state.committed_action().to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::{hierarchy::ChildOf, prelude::*, schedule::Schedule};

    use super::*;
    use crate::resources::protagonist_action::ProtagonistDecisionState;

    #[test]
    fn records_initial_world_snapshot_as_round_one() {
        let mut world = World::new();
        world.spawn((
            TurnFlow {
                turn_index: 0,
                active_turn_id: 1,
                stage: TurnStage::Simulation,
            },
            WorldSnapshot {
                round: 1,
                scene_title: "opening".to_string(),
                ..WorldSnapshot::default()
            },
            ProtagonistDecisionState::default(),
            SessionHistoryLog::default(),
        ));
        let mut schedule = Schedule::default();
        schedule.add_systems(history_sys);

        schedule.run(&mut world);

        let history = world
            .query::<&SessionHistoryLog>()
            .single(&world)
            .expect("session history should exist");
        assert_eq!(history.rounds.len(), 1);
        assert_eq!(history.rounds[0].round, 1);
        assert_eq!(
            history.rounds[0]
                .world_snapshot
                .as_ref()
                .map(|snapshot| snapshot.round),
            Some(1)
        );
    }

    #[test]
    fn does_not_record_idle_default_snapshot() {
        let mut world = World::new();
        world.spawn((
            TurnFlow::default(),
            WorldSnapshot::default(),
            ProtagonistDecisionState::default(),
            SessionHistoryLog::default(),
        ));
        let mut schedule = Schedule::default();
        schedule.add_systems(history_sys);

        schedule.run(&mut world);

        let history = world
            .query::<&SessionHistoryLog>()
            .single(&world)
            .expect("session history should exist");
        assert!(history.rounds.is_empty());
    }

    #[test]
    fn records_initial_round_one_narration() {
        let mut world = World::new();
        let session = world
            .spawn((
                TurnFlow {
                    turn_index: 1,
                    active_turn_id: 1,
                    stage: TurnStage::TurnCompleted,
                },
                WorldSnapshot {
                    round: 1,
                    ..WorldSnapshot::default()
                },
                ProtagonistDecisionState::default(),
                SessionHistoryLog::default(),
            ))
            .id();
        world.spawn((
            ChildOf(session),
            NarrationOutcome {
                turn_id: 1,
                content: "opening narration".to_string(),
            },
        ));
        let mut schedule = Schedule::default();
        schedule.add_systems(history_sys);

        schedule.run(&mut world);

        let history = world
            .query::<&SessionHistoryLog>()
            .single(&world)
            .expect("session history should exist");
        assert_eq!(
            history.rounds[0].narration_text.as_deref(),
            Some("opening narration")
        );
    }
}
