//! The panel shown inside SteamVR. Pure egui: reads `State`, returns commands for the server.

use crate::server::{Command, State};
use alvr_gui_common::{i18n::tr, theme};
use egui::{Color32, RichText, Ui};
use serde_json as json;
use std::time::Duration;

const BITRATE_MODE: &str = "session_settings.video.bitrate.mode.variant";
const CONSTANT_BITRATE: &str = "session_settings.video.bitrate.mode.ConstantMbps";
const MAX_BITRATE_ENABLED: &str = "session_settings.video.bitrate.mode.Adaptive.max_throughput_mbps.enabled";
const MAX_BITRATE: &str = "session_settings.video.bitrate.mode.Adaptive.max_throughput_mbps.content";
const CODEC: &str = "session_settings.video.preferred_codec.variant";
// Same settings the dashboard "Resolution" preset writes: the encoded and the emulated headset
// resolution are kept equal
const RESOLUTION_PATHS: [&str; 2] = [
    "session_settings.video.transcoding_view_resolution",
    "session_settings.video.emulated_headset_view_resolution",
];
const RESOLUTION_MODE: &str = "session_settings.video.transcoding_view_resolution.variant";
const RESOLUTION_WIDTH: &str = "session_settings.video.transcoding_view_resolution.Absolute.width";
const FPS: &str = "session_settings.video.preferred_fps";

const BITRATE_STEP_MBPS: u64 = 10;
const BITRATE_RANGE_MBPS: (u64, u64) = (10, 500);
const DEFAULT_MAX_BITRATE_MBPS: u64 = 100;
const STATS_TIMEOUT: Duration = Duration::from_secs(3);

// (label, session variant)
const BITRATE_MODES: [(&str, &str); 2] = [("Constant", "ConstantMbps"), ("Adaptive", "Adaptive")];
const CODECS: [(&str, &str); 3] = [("H264", "H264"), ("HEVC", "Hevc"), ("AV1", "AV1")];
// (both eyes width as the dashboard shows it, per eye width as stored in the session)
const RESOLUTIONS: [(&str, u64); 6] = [
    ("3072", 1536),
    ("3712", 1856),
    ("4288", 2144),
    ("5184", 2592),
    ("5632", 2816),
    ("6080", 3040),
];
// Same as the dashboard preset
const FRAMERATES: [u64; 5] = [60, 72, 80, 90, 120];
const RUNNING_FPS: &str = "steamvr_hmd_init_config.refresh_rate";

fn get<'a>(session: &'a json::Value, path: &str) -> Option<&'a json::Value> {
    path.split('.').try_fold(session, |value, key| value.get(key))
}

fn set(commands: &mut Vec<Command>, path: &str, value: impl Into<json::Value>) {
    commands.push(Command::SetValue {
        path: path.into(),
        value: value.into(),
    });
}

const CARD_BG: Color32 = Color32::from_rgb(34, 35, 42);
const MUTED: Color32 = Color32::from_gray(150);
const BAD_RED: Color32 = Color32::from_rgb(235, 87, 87);
const BUTTON_HEIGHT: f32 = 56.0;
const TILE_COLUMNS: usize = 4;
const TILE_HEIGHT: f32 = 62.0;

// A titled box. `needs_restart` flags settings that SteamVR only reads at startup.
fn card(ui: &mut Ui, text: &str, needs_restart: bool, content: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(CARD_BG)
        .corner_radius(14.0)
        .inner_margin(18.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr(text)).size(20.0).strong());
                if needs_restart {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(tr("⚠ Applied after SteamVR restarts"))
                                .color(theme::log_colors::WARNING_LIGHT)
                                .size(15.0),
                        );
                    });
                }
            });
            ui.add_space(6.0);
            content(ui);
        });
}

// Exclusive choices as equal width buttons, `per_row` per line. Returns the clicked index.
fn choice_grid(
    ui: &mut Ui,
    selected: Option<usize>,
    labels: &[String],
    per_row: usize,
) -> Option<usize> {
    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - gap * (per_row - 1) as f32) / per_row as f32;
    let mut clicked = None;

    for (row, chunk) in labels.chunks(per_row).enumerate() {
        ui.horizontal(|ui| {
            for (i, label) in chunk.iter().enumerate() {
                let index = row * per_row + i;
                let button = egui::Button::selectable(
                    selected == Some(index),
                    RichText::new(label).size(19.0),
                )
                .frame_when_inactive(true);
                if ui.add_sized([width, BUTTON_HEIGHT], button).clicked() {
                    clicked = Some(index);
                }
            }
        });
    }
    clicked
}

// "−  value  ＋" row. Returns the new value when a button is clicked.
fn stepper(ui: &mut Ui, shown: &str, current: u64) -> Option<u64> {
    let mut new_value = None;
    let gap = ui.spacing().item_spacing.x;
    let side = 84.0;
    let middle = ui.available_width() - 2.0 * side - 2.0 * gap;

    ui.horizontal(|ui| {
        let button = |text: &str| egui::Button::new(RichText::new(text).size(24.0));
        if ui.add_sized([side, BUTTON_HEIGHT], button("−")).clicked() {
            new_value = Some(current.saturating_sub(BITRATE_STEP_MBPS));
        }
        ui.add_sized(
            [middle, BUTTON_HEIGHT],
            egui::Label::new(RichText::new(shown).size(26.0).strong()),
        );
        if ui.add_sized([side, BUTTON_HEIGHT], button("＋")).clicked() {
            new_value = Some(current + BITRATE_STEP_MBPS);
        }
    });
    new_value.map(|v| v.clamp(BITRATE_RANGE_MBPS.0, BITRATE_RANGE_MBPS.1))
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED).size(15.0));
}

fn resolution_commands(width_per_eye: u64) -> Vec<Command> {
    let mut commands = vec![];
    for base in RESOLUTION_PATHS {
        set(&mut commands, &format!("{base}.variant"), "Absolute");
        set(&mut commands, &format!("{base}.Absolute.width"), width_per_eye);
        set(&mut commands, &format!("{base}.Absolute.height.set"), false);
    }
    commands
}

fn header(ui: &mut Ui, state: &State) {
    let size = egui::vec2(ui.available_width(), 52.0);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(RichText::new("ALVR").size(34.0).strong());
        let (text, color) = match (&state.connected, &state.stats) {
            (false, _) => (tr("Server not running"), Color32::GRAY),
            (true, Some((at, _))) if at.elapsed() < STATS_TIMEOUT => {
                (tr("Streaming"), theme::OK_GREEN)
            }
            (true, _) => (tr("Waiting for a headset"), theme::log_colors::WARNING_LIGHT),
        };
        egui::Frame::new()
            .fill(color.gamma_multiply(0.18))
            .corner_radius(20.0)
            .inner_margin(egui::Margin::symmetric(14, 4))
            .show(ui, |ui| {
                ui.label(RichText::new(format!("● {text}")).color(color).size(18.0));
            });
    });
}

pub fn panel(ui: &mut Ui, state: &State) -> Vec<Command> {
    let mut commands = vec![];

    header(ui, state);
    ui.add_space(8.0);

    let Some(session) = &state.session else {
        ui.label(tr("Waiting for the ALVR server..."));
        return commands;
    };

    if let Some((at, stats)) = &state.stats
        && at.elapsed() < STATS_TIMEOUT
    {
        stat_tiles(ui, stats);
        ui.add_space(8.0);
    }

    ui.columns(2, |columns| {
        let [left, right] = columns else { unreachable!() };

        card(left, "Bitrate mode", false, |ui| {
            let mode = get(session, BITRATE_MODE).and_then(|v| v.as_str());
            let selected = BITRATE_MODES.iter().position(|(_, variant)| Some(*variant) == mode);
            let labels: Vec<_> = BITRATE_MODES.iter().map(|(label, _)| tr(label)).collect();
            if let Some(i) = choice_grid(ui, selected, &labels, 2) {
                set(&mut commands, BITRATE_MODE, BITRATE_MODES[i].1);
            }

            ui.add_space(10.0);
            match mode {
                Some("ConstantMbps") => {
                    let current =
                        get(session, CONSTANT_BITRATE).and_then(|v| v.as_u64()).unwrap_or(0);
                    caption(ui, &tr("Bitrate"));
                    if let Some(value) = stepper(ui, &format!("{current} Mbps"), current) {
                        set(&mut commands, CONSTANT_BITRATE, value);
                    }
                }
                Some("Adaptive") => {
                    let enabled =
                        get(session, MAX_BITRATE_ENABLED).and_then(|v| v.as_bool()) == Some(true);
                    let current = get(session, MAX_BITRATE)
                        .and_then(|v| v.as_u64())
                        .unwrap_or(DEFAULT_MAX_BITRATE_MBPS);
                    let shown = if enabled { format!("{current} Mbps") } else { tr("Unlimited") };
                    caption(ui, &tr("Maximum bitrate"));
                    if let Some(value) = stepper(ui, &shown, current) {
                        set(&mut commands, MAX_BITRATE_ENABLED, true);
                        set(&mut commands, MAX_BITRATE, value);
                    }
                }
                _ => (),
            }
        });

        left.add_space(8.0);
        card(left, "Codec", false, |ui| {
            let codec = get(session, CODEC).and_then(|v| v.as_str());
            let selected = CODECS.iter().position(|(_, variant)| Some(*variant) == codec);
            let labels: Vec<_> = CODECS.iter().map(|(label, _)| label.to_string()).collect();
            if let Some(i) = choice_grid(ui, selected, &labels, 3) {
                set(&mut commands, CODEC, CODECS[i].1);
            }
        });

        if state.stats.is_some() {
            left.add_space(8.0);
            let button = egui::Button::new(RichText::new(tr("Insert IDR")).size(19.0));
            let width = left.available_width();
            if left.add_sized([width, BUTTON_HEIGHT], button).clicked() {
                commands.push(Command::InsertIdr);
            }
        }

        card(right, "Resolution (width)", true, |ui| {
            let width =
                (get(session, RESOLUTION_MODE).and_then(|v| v.as_str()) == Some("Absolute"))
                    .then(|| get(session, RESOLUTION_WIDTH).and_then(|v| v.as_u64()))
                    .flatten();
            let selected = RESOLUTIONS.iter().position(|(_, w)| Some(*w) == width);
            let labels: Vec<_> = RESOLUTIONS.iter().map(|(label, _)| label.to_string()).collect();
            if let Some(i) = choice_grid(ui, selected, &labels, 3) {
                commands.extend(resolution_commands(RESOLUTIONS[i].1));
            }
        });

        right.add_space(8.0);
        card(right, "Frame rate", true, |ui| {
            let fps = get(session, FPS).and_then(|v| v.as_f64());
            let selected = FRAMERATES.iter().position(|f| Some(*f as f64) == fps);
            let labels: Vec<_> = FRAMERATES.iter().map(|f| format!("{f} Hz")).collect();
            if let Some(i) = choice_grid(ui, selected, &labels, 3) {
                set(&mut commands, FPS, FRAMERATES[i] as f64);
            }
            running_fps(ui, session, fps);
        });
    });

    commands
}

// What SteamVR runs at can differ from the setting: it is read when SteamVR starts, and the client
// may not support the requested rate
fn running_fps(ui: &mut Ui, session: &json::Value, setting: Option<f64>) {
    let Some(running) = get(session, RUNNING_FPS).and_then(|v| v.as_f64()) else {
        return;
    };

    ui.add_space(6.0);
    let mismatch = setting.is_some_and(|setting| (setting - running).abs() > 0.5);
    let text = format!("{} {running:.0} Hz", tr("Current SteamVR refresh rate:"));
    if mismatch {
        ui.label(
            RichText::new(format!("{text} — {}", tr("Applied after reconnecting")))
                .color(theme::log_colors::WARNING_LIGHT)
                .size(15.0),
        );
    } else {
        caption(ui, &text);
    }
}

// Big value on top of a small label; the value is colored when it is worth a look
fn tile(ui: &mut Ui, width: f32, label: &str, value: String, unit: &str, color: Color32) {
    egui::Frame::new()
        .fill(CARD_BG)
        .corner_radius(12.0)
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.set_min_height(TILE_HEIGHT);
            ui.vertical(|ui| {
                ui.label(RichText::new(label.trim_end_matches(':')).color(MUTED).size(15.0));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(value).size(30.0).strong().color(color));
                    ui.label(RichText::new(unit).color(MUTED));
                });
            });
        });
}

fn stat_tiles(ui: &mut Ui, stats: &alvr_events::StatisticsSummary) {
    let normal = theme::FG;
    // Thresholds are rough: they only draw the eye to a number
    let level = |value: f32, warn: f32, bad: f32| {
        if value >= bad {
            BAD_RED
        } else if value >= warn {
            theme::log_colors::WARNING_LIGHT
        } else {
            normal
        }
    };

    let tiles = [
        (tr("Bitrate:"), format!("{:.1}", stats.video_mbits_per_sec), "Mbps", normal),
        (
            tr("Total latency:"),
            format!("{:.0}", stats.total_latency_ms),
            "ms",
            level(stats.total_latency_ms, 60.0, 100.0),
        ),
        (
            tr("Packet loss:"),
            format!("{:.2}", stats.video_packet_loss_percent),
            "%",
            level(stats.video_packet_loss_percent, 1.0, 5.0),
        ),
        (
            tr("Transport latency:"),
            format!("{:.1}", stats.network_latency_ms),
            "ms",
            level(stats.network_latency_ms, 20.0, 40.0),
        ),
        (
            tr("Encoder latency:"),
            format!("{:.1}", stats.encode_latency_ms),
            "ms",
            level(stats.encode_latency_ms, 15.0, 30.0),
        ),
        (
            tr("Decoder latency:"),
            format!("{:.1}", stats.decode_latency_ms),
            "ms",
            level(stats.decode_latency_ms, 15.0, 30.0),
        ),
        (tr("Client FPS:"), format!("{}", stats.client_fps), "fps", normal),
        (tr("Streamer FPS:"), format!("{}", stats.server_fps), "fps", normal),
    ];

    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - gap * (TILE_COLUMNS - 1) as f32) / TILE_COLUMNS as f32;
    for row in tiles.chunks(TILE_COLUMNS) {
        ui.horizontal(|ui| {
            for (label, value, unit, color) in row {
                tile(ui, width, label, value.clone(), unit, *color);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The panel edits the session by path: fail loudly if a setting is moved or renamed upstream.
    #[test]
    fn session_paths_exist() {
        let session = json::to_value(alvr_session::SessionConfig::default()).unwrap();

        for path in [BITRATE_MODE, CODEC, RESOLUTION_MODE, FPS, RUNNING_FPS] {
            assert!(get(&session, path).is_some(), "missing {path}");
        }
        for path in [CONSTANT_BITRATE, MAX_BITRATE_ENABLED, MAX_BITRATE, RESOLUTION_WIDTH] {
            assert!(get(&session, path).is_some(), "missing {path}");
        }
        for base in RESOLUTION_PATHS {
            for suffix in ["variant", "Absolute.width", "Absolute.height.set"] {
                let path = format!("{base}.{suffix}");
                assert!(get(&session, &path).is_some(), "missing {path}");
            }
        }
    }

    // Applies the commands the way the server does and reads the result back
    #[test]
    fn resolution_matches_dashboard_preset() {
        let mut manager = alvr_server_io::ServerSessionManager::new(None);

        let pairs = resolution_commands(RESOLUTIONS[2].1)
            .into_iter()
            .map(|command| match command {
                Command::SetValue { path, value } => alvr_packets::PathValuePair {
                    path: alvr_packets::parse_path(&path).unwrap(),
                    value,
                },
                Command::InsertIdr => unreachable!(),
            })
            .collect();
        manager.set_session_values(pairs).unwrap();

        let session = json::to_value(manager.session().clone()).unwrap();
        for base in RESOLUTION_PATHS {
            assert_eq!(get(&session, &format!("{base}.variant")), Some(&json::json!("Absolute")));
            assert_eq!(get(&session, &format!("{base}.Absolute.width")), Some(&json::json!(2144)));
            assert_eq!(get(&session, &format!("{base}.Absolute.height.set")), Some(&json::json!(false)));
        }
    }

    #[test]
    fn simple_settings_are_applied() {
        let mut manager = alvr_server_io::ServerSessionManager::new(None);
        let pair = |path: &str, value: json::Value| alvr_packets::PathValuePair {
            path: alvr_packets::parse_path(path).unwrap(),
            value,
        };

        manager
            .set_session_values(vec![
                pair(CODEC, json::json!("Hevc")),
                pair(FPS, json::json!(90.0)),
                pair(BITRATE_MODE, json::json!("Adaptive")),
                pair(MAX_BITRATE_ENABLED, json::json!(true)),
                pair(MAX_BITRATE, json::json!(150)),
            ])
            .unwrap();

        let settings = manager.settings();
        assert!(matches!(settings.video.preferred_codec, alvr_session::CodecType::Hevc));
        assert_eq!(settings.video.preferred_fps, 90.0);
        let alvr_session::BitrateMode::Adaptive { max_throughput_mbps, .. } =
            &settings.video.bitrate.mode
        else {
            panic!("not adaptive");
        };
        assert_eq!(max_throughput_mbps.as_option(), Some(&150));
    }
}
