use alvr_gui_common::theme;
use eframe::egui::{Button, Response, RichText, Ui, vec2};
use super::NestingInfo;
use alvr_gui_common::i18n::tr;
use alvr_packets::PathValuePair;
use serde_json as json;

const HEADER_HEIGHT: f32 = 36.0;
const HEADER_MIN_WIDTH: f32 = 260.0;

pub fn is_collapsed(session_fragment: &json::Value) -> bool {
    session_fragment
        .get("gui_collapsed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn set_collapsed(session_fragment: &mut json::Value, collapsed: bool) {
    session_fragment["gui_collapsed"] = json::Value::Bool(collapsed);
}

// The whole label is the click target, so a collapsible section reads as a clickable heading
// instead of a small button far from its name.
pub fn header(ui: &mut Ui, text: &str, collapsed: bool) -> Response {
    let arrow = if collapsed { "▶" } else { "▼" };
    ui.add(
        Button::new(RichText::new(format!("{arrow}  {text}")).strong().size(15.0))
            .fill(theme::SEPARATOR_BG.gamma_multiply(0.6))
            .min_size(vec2(HEADER_MIN_WIDTH, HEADER_HEIGHT)),
    )
}

// Toggle for the controls that fold their own content (array, vector, dictionary). Returns the
// collapsed state.
pub fn collapsible_button(
    ui: &mut Ui,
    nesting_info: &NestingInfo,
    session_fragment: &mut json::Value,
    request: &mut Option<PathValuePair>,
) -> bool {
    let collapsed = is_collapsed(session_fragment);
    let text = if collapsed { tr("Expand") } else { tr("Collapse") };

    if header(ui, &text, collapsed).clicked() {
        set_collapsed(session_fragment, !collapsed);
        *request = super::get_single_value(
            nesting_info,
            "gui_collapsed".into(),
            json::Value::Bool(!collapsed),
        );
    }

    is_collapsed(session_fragment)
}
