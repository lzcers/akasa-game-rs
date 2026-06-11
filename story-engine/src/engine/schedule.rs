use bevy_ecs::{
    message::message_update_system,
    schedule::{IntoScheduleConfigs, Schedule, SystemSet},
};

use crate::systems::{
    agents::{
        fate_weaver_sys::{fate_weaver_apply_system, fate_weaver_dispatch_system},
        narration_sys::{narration_apply_system, narration_dispatch_system},
        player_sys::player_input_consume_system,
        protagonist_sys::{protagonist_apply_system, protagonist_dispatch_system},
    },
    flow::{agent_task_system, cleanup_previous_turn_outcomes_system, flow_progress_system},
};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StoryScheduleSet {
    Dispatch,
    PollTasks,
    ApplyResults,
    Progress,
    Finalize,
}

pub(crate) fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.configure_sets(
        (
            StoryScheduleSet::Dispatch,
            StoryScheduleSet::PollTasks,
            StoryScheduleSet::ApplyResults,
            StoryScheduleSet::Progress,
            StoryScheduleSet::Finalize,
        )
            .chain(),
    );
    schedule.add_systems(
        (
            fate_weaver_dispatch_system,
            narration_dispatch_system,
            protagonist_dispatch_system,
        )
            .in_set(StoryScheduleSet::Dispatch),
    );
    schedule.add_systems(agent_task_system.in_set(StoryScheduleSet::PollTasks));
    schedule.add_systems(
        (
            fate_weaver_apply_system,
            narration_apply_system,
            protagonist_apply_system,
            player_input_consume_system,
        )
            .in_set(StoryScheduleSet::ApplyResults),
    );
    schedule.add_systems(flow_progress_system.in_set(StoryScheduleSet::Progress));
    schedule.add_systems(
        (cleanup_previous_turn_outcomes_system, message_update_system)
            .chain()
            .in_set(StoryScheduleSet::Finalize),
    );
    schedule
}
