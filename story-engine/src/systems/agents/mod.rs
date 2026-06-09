use bevy_ecs::entity::Entity;

use crate::{
    components::{session::StorySession, turn_flow::TurnFlow},
    engine::RuntimeDebugObserverResource,
    resources::{
        agent_task::{TaskKind, TaskStatus, TaskUpdate},
        export::ExportState,
    },
};

pub mod fate_weaver_sys;
pub mod narration_sys;
pub mod player_sys;
pub mod protagonist_sys;

fn publish_apply_error(
    export_state: &ExportState,
    debug_observer: Option<&RuntimeDebugObserverResource>,
    session: Option<&StorySession>,
    flow: &TurnFlow,
    entity: Entity,
    kind: TaskKind,
    error: String,
) {
    let update = TaskUpdate {
        entity: format!("{entity:?}"),
        kind,
        status: TaskStatus::Error,
        chunk_kind: None,
        chunk: None,
        output: None,
        error: Some(error),
    };
    if let (Some(session), Some(observer)) = (
        session,
        debug_observer.and_then(|debug| debug.observer.as_ref()),
    ) {
        observer.on_task_update(&session.id, flow.active_turn_id.max(1), &update);
    }
    export_state.publish_task_update(flow.active_turn_id.max(1), update);
}

fn output_preview(output: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 240;
    let trimmed = output.trim();
    let mut preview: String = trimmed.chars().take(MAX_PREVIEW_CHARS).collect();
    if trimmed.chars().count() > MAX_PREVIEW_CHARS {
        preview.push_str("...");
    }
    preview
}
