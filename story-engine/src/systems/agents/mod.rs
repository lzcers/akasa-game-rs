use crate::components::{session_event_sink::SessionEventSink, turn_flow::TurnFlow};

pub mod fate_weaver_sys;
pub mod narration_sys;
pub mod player_sys;
pub mod protagonist_sys;

fn publish_apply_error(
    event_sink: &SessionEventSink,
    flow: &TurnFlow,
    entity_name: &str,
    error: String,
) {
    event_sink.publish_flow_turn_error(
        flow.active_turn_id.max(1),
        flow.stage,
        entity_name.to_string(),
        error,
    );
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
