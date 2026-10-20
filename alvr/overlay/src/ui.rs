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

// "−  value  ＋" row. Returns the new value when a button is clicked.
fn stepper(ui: &mut Ui, label: &str, shown: &str, current: u64) -> Option<u64> {
    let mut new_value = None;
    ui.horizontal(|ui| {
        ui.label(tr(label));
        let size = egui::vec2(60.0, 44.0);
        if ui.add(egui::Button::new("−").min_size(size)).clicked() {
            new_value = Some(current.saturating_sub(BITRATE_STEP_MBPS));
        }
        ui.label(shown);
        if ui.add(egui::Button::new("＋").min_size(size)).clicked() {
            new_value = Some(current + BITRATE_STEP_MBPS);
        }
    });
    new_value.map(|v| v.clamp(BITRATE_RANGE_MBPS.0, BITRATE_RANGE_MBPS.1))
}

// One row of exclusive choices. Returns the clicked index.
fn choice_row(ui: &mut Ui, selected: Option<usize>, labels: &[String]) -> Option<usize> {
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for (i, label) in labels.iter().enumerate() {
            let button = egui::Button::selectable(selected == Some(i), label)
                .frame_when_inactive(true)
                .min_size(egui::vec2(96.0, 44.0));
            if ui.add(button).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

// Section title, optionally flagging settings that SteamVR only reads at startup
fn title(ui: &mut Ui, text: &str, needs_restart: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr(text)).strong());
        if needs_restart {
            ui.label(
                RichText::new(tr("⚠ Applied after SteamVR restarts"))
                    .color(theme::log_colors::WARNING_LIGHT)
                    .small(),
            );
        }
    });
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

pub fn panel(ui: &mut Ui, state: &State) -> Vec<Command> {
    let mut commands = vec![];

    ui.horizontal(|ui| {
        ui.heading("ALVR");
        let (text, color) = match (&state.connected, &state.stats) {
            (false, _) => (tr("Server not running"), Color32::GRAY),
            (true, Some((at, _))) if at.elapsed() < STATS_TIMEOUT => {
                (tr("Streaming"), theme::OK_GREEN)
            }
            (true, _) => (tr("Waiting for a headset"), theme::log_colors::WARNING_LIGHT),
        };
        ui.label(RichText::new(format!("● {text}")).color(color));
    });

    let Some(session) = &state.session else {
        ui.label(tr("Waiting for the ALVR server..."));
        return commands;
    };

    if let Some((at, stats)) = &state.stats
        && at.elapsed() < STATS_TIMEOUT
    {
        stats_grid(ui, stats);
    }

    ui.separator();

    // Bitrate: both modes are applied in real time
    title(ui, "Bitrate mode", false);
    let mode = get(session, BITRATE_MODE).and_then(|v| v.as_str());
    let selected = BITRATE_MODES.iter().position(|(_, variant)| Some(*variant) == mode);
    let labels: Vec<_> = BITRATE_MODES.iter().map(|(label, _)| tr(label)).collect();
    if let Some(i) = choice_row(ui, selected, &labels) {
        set(&mut commands, BITRATE_MODE, BITRATE_MODES[i].1);
    }

    match mode {
        Some("ConstantMbps") => {
            let current = get(session, CONSTANT_BITRATE).and_then(|v| v.as_u64()).unwrap_or(0);
            if let Some(value) = stepper(ui, "Bitrate", &format!("{current} Mbps"), current) {
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
            if let Some(value) = stepper(ui, "Maximum bitrate", &shown, current) {
                set(&mut commands, MAX_BITRATE_ENABLED, true);
                set(&mut commands, MAX_BITRATE, value);
            }
        }
        _ => (),
    }

    title(ui, "Codec", false);
    let codec = get(session, CODEC).and_then(|v| v.as_str());
    let selected = CODECS.iter().position(|(_, variant)| Some(*variant) == codec);
    let labels: Vec<_> = CODECS.iter().map(|(label, _)| label.to_string()).collect();
    if let Some(i) = choice_row(ui, selected, &labels) {
        set(&mut commands, CODEC, CODECS[i].1);
    }

    title(ui, "Resolution (width)", true);
    let width = (get(session, RESOLUTION_MODE).and_then(|v| v.as_str()) == Some("Absolute"))
        .then(|| get(session, RESOLUTION_WIDTH).and_then(|v| v.as_u64()))
        .flatten();
    let selected = RESOLUTIONS.iter().position(|(_, w)| Some(*w) == width);
    let labels: Vec<_> = RESOLUTIONS.iter().map(|(label, _)| label.to_string()).collect();
    if let Some(i) = choice_row(ui, selected, &labels) {
        commands.extend(resolution_commands(RESOLUTIONS[i].1));
    }

    title(ui, "Frame rate", true);
    let fps = get(session, FPS).and_then(|v| v.as_f64());
    let selected = FRAMERATES.iter().position(|f| Some(*f as f64) == fps);
    let labels: Vec<_> = FRAMERATES.iter().map(|f| format!("{f} Hz")).collect();
    if let Some(i) = choice_row(ui, selected, &labels) {
        set(&mut commands, FPS, FRAMERATES[i] as f64);
    }
    running_fps(ui, session, fps);

    ui.separator();
    if state.stats.is_some() && ui.button(tr("Insert IDR")).clicked() {
        commands.push(Command::InsertIdr);
    }

    commands
}

// What SteamVR runs at can differ from the setting: it is read when SteamVR starts, and the client
// may not support the requested rate
fn running_fps(ui: &mut Ui, session: &json::Value, setting: Option<f64>) {
    let Some(running) = get(session, RUNNING_FPS).and_then(|v| v.as_f64()) else {
        return;
    };

    let mismatch = setting.is_some_and(|setting| (setting - running).abs() > 0.5);
    let text = format!("{} {running:.0} Hz", tr("Current SteamVR refresh rate:"));
    if mismatch {
        ui.label(
            RichText::new(format!("{text} — {}", tr("Applied after reconnecting")))
                .color(theme::log_colors::WARNING_LIGHT)
                .small(),
        );
    } else {
        ui.label(RichText::new(text).small());
    }
}

fn stats_grid(ui: &mut Ui, stats: &alvr_events::StatisticsSummary) {
    egui::Grid::new("stats").num_columns(4).spacing([24.0, 8.0]).show(ui, |ui| {
        let mut row = |a: String, va: String, b: String, vb: String| {
            ui.label(a);
            ui.label(va);
            ui.label(b);
            ui.label(vb);
            ui.end_row();
        };

        row(
            tr("Bitrate:"),
            format!("{:.1} Mbps", stats.video_mbits_per_sec),
            tr("Total latency:"),
            format!("{:.0} ms", stats.total_latency_ms),
        );
        row(
            tr("Transport latency:"),
            format!("{:.1} ms", stats.network_latency_ms),
            tr("Encoder latency:"),
            format!("{:.1} ms", stats.encode_latency_ms),
        );
        row(
            tr("Decoder latency:"),
            format!("{:.1} ms", stats.decode_latency_ms),
            tr("Packet loss:"),
            format!("{:.2} %", stats.video_packet_loss_percent),
        );
        row(
            tr("Client FPS:"),
            format!("{}", stats.client_fps),
            tr("Streamer FPS:"),
            format!("{}", stats.server_fps),
        );
    });
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
