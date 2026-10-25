use alvr_gui_common::i18n::tr;
use super::{
    NestingInfo, SettingControl,
    presets::{PresetControl, builtin_schema},
    set_filter,
};
use crate::dashboard::ServerRequest;
use alvr_gui_common::{DisplayString, theme};
use alvr_session::{SessionSettings, Settings};
use alvr_packets::PathValuePair;
use eframe::egui::{
    Button, CentralPanel, FontId, Frame, Grid, Margin, Panel, RichText, ScrollArea,
    TextEdit, Ui,
};
#[cfg(target_arch = "wasm32")]
use instant::Instant;
use serde_json as json;
use settings_schema::SchemaNode;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

const DATA_UPDATE_INTERVAL: Duration = Duration::from_secs(1);
const MIN_COLUMN_SIZE: f32 = 320.0;
const MAX_CONTENT_WIDTH: f32 = 1100.0;
const NAV_WIDTH: f32 = 220.0;
const NAV_BUTTON_HEIGHT: f32 = 44.0;

struct TopLevelEntry {
    id: DisplayString,
    control: SettingControl,
}

pub struct SettingsTab {
    selected_top_tab_id: String,
    search: String,
    resolution_preset: PresetControl,
    framerate_preset: PresetControl,
    encoder_preset: PresetControl,
    foveation_preset: PresetControl,
    codec_preset: PresetControl,
    game_audio_preset: PresetControl,
    microphone_preset: PresetControl,
    hand_tracking_interaction_preset: PresetControl,
    eye_face_tracking_preset: PresetControl,
    top_level_entries: Vec<TopLevelEntry>,
    session_settings_json: Option<json::Value>,
    last_update_instant: Instant,
}

impl SettingsTab {
    pub fn new() -> Self {
        let nesting_info = NestingInfo {
            path: vec!["session_settings".into()],
            indentation_level: 0,
        };
        let schema = Settings::schema(alvr_session::session_settings_default());

        // Top level node must be a section
        let SchemaNode::Section { entries, .. } = schema else {
            unreachable!();
        };

        let top_level_entries = entries
            .into_iter()
            .map(|entry| {
                let id = entry.name;
                let display = super::get_display_name(&id, &entry.strings);

                let mut nesting_info = nesting_info.clone();
                nesting_info.path.push(id.clone().into());

                TopLevelEntry {
                    id: DisplayString { id, display },
                    control: SettingControl::new(nesting_info, entry.content),
                }
            })
            .collect();

        Self {
            selected_top_tab_id: "presets".into(),
            search: String::new(),
            resolution_preset: PresetControl::new(builtin_schema::resolution_schema()),
            framerate_preset: PresetControl::new(builtin_schema::framerate_schema()),
            encoder_preset: PresetControl::new(builtin_schema::encoder_preset_schema()),
            foveation_preset: PresetControl::new(builtin_schema::foveation_preset_schema()),
            codec_preset: PresetControl::new(builtin_schema::codec_preset_schema()),
            game_audio_preset: PresetControl::new(builtin_schema::game_audio_schema()),
            microphone_preset: PresetControl::new(builtin_schema::microphone_schema()),
            hand_tracking_interaction_preset: PresetControl::new(
                builtin_schema::hand_tracking_interaction_schema(),
            ),
            eye_face_tracking_preset: PresetControl::new(builtin_schema::eye_face_tracking_schema()),
            top_level_entries,
            session_settings_json: None,
            last_update_instant: Instant::now(),
        }
    }

    #[cfg(test)]
    pub fn set_search(&mut self, text: &str) {
        self.search = text.into();
    }

    #[cfg(test)]
    pub fn select_tab(&mut self, id: &str) {
        self.selected_top_tab_id = id.into();
    }

    pub fn update_session(&mut self, session_settings: &SessionSettings) {
        let settings_json = json::to_value(session_settings).unwrap();

        self.resolution_preset
            .update_session_settings(&settings_json);
        self.framerate_preset
            .update_session_settings(&settings_json);
        self.encoder_preset.update_session_settings(&settings_json);
        self.foveation_preset
            .update_session_settings(&settings_json);
        self.codec_preset.update_session_settings(&settings_json);
        self.game_audio_preset
            .update_session_settings(&settings_json);
        self.microphone_preset
            .update_session_settings(&settings_json);
        self.hand_tracking_interaction_preset
            .update_session_settings(&settings_json);
        self.eye_face_tracking_preset
            .update_session_settings(&settings_json);

        self.session_settings_json = Some(settings_json);
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Vec<ServerRequest> {
        let mut requests = vec![];

        let now = Instant::now();
        if now > self.last_update_instant + DATA_UPDATE_INTERVAL {
            if self.session_settings_json.is_none() {
                requests.push(ServerRequest::GetSession);
            }

            self.last_update_instant = now;
        }

        Panel::left("settings_nav")
            .resizable(false)
            .exact_size(NAV_WIDTH)
            .frame(
                Frame::new()
                    .fill(theme::DARKER_BG)
                    .corner_radius(theme::CORNER_RADIUS)
                    .inner_margin(theme::FRAME_PADDING),
            )
            .show(ui, |ui| self.nav_ui(ui));

        let mut path_value_pairs = vec![];
        CentralPanel::default()
            .frame(Frame::new().inner_margin(Margin {
                left: 20,
                right: 0,
                top: 0,
                bottom: 0,
            }))
            .show(ui, |ui| path_value_pairs = self.content_ui(ui));

        if !path_value_pairs.is_empty() {
            requests.push(ServerRequest::SetSessionValues(path_value_pairs));
        }

        requests
    }

    fn query(&self) -> Option<String> {
        let query = self.search.trim().to_lowercase();
        (!query.is_empty()).then_some(query)
    }

    // Search box and the category list, as large buttons
    fn nav_ui(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 8.0;

        ui.add(
            TextEdit::singleline(&mut self.search)
                .hint_text(tr("Search settings"))
                .font(FontId::proportional(16.0))
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(10, 10)),
        );
        ui.add_space(6.0);

        let searching = self.query().is_some();
        let entries = std::iter::once(("presets".to_owned(), tr("Presets"))).chain(
            self.top_level_entries
                .iter()
                .map(|entry| (entry.id.id.clone(), entry.id.display.clone())),
        );

        for (id, display) in entries {
            let selected = !searching && self.selected_top_tab_id == id;
            let button = Button::selectable(selected, RichText::new(display).size(16.0));
            if ui
                .add_sized([ui.available_width(), NAV_BUTTON_HEIGHT], button)
                .clicked()
            {
                self.selected_top_tab_id = id;
                self.search.clear();
            }
        }
    }

    fn content_ui(&mut self, ui: &mut Ui) -> Vec<PathValuePair> {
        let mut path_value_pairs = vec![];
        let query = self.query();
        ui.set_max_width(MAX_CONTENT_WIDTH);

        let title = match &query {
            Some(_) => tr("Search results"),
            None if self.selected_top_tab_id == "presets" => tr("Presets"),
            None => self
                .top_level_entries
                .iter()
                .find(|entry| entry.id.id == self.selected_top_tab_id)
                .map(|entry| entry.id.display.clone())
                .unwrap_or_default(),
        };

        // Expand / collapse every section of the page at once
        let mut collapse_all = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(22.0).strong());

            if query.is_none() && self.selected_top_tab_id != "presets" {
                ui.add_space(20.0);
                if ui.button(tr("Expand all")).clicked() {
                    collapse_all = Some(false);
                }
                if ui.button(tr("Collapse all")).clicked() {
                    collapse_all = Some(true);
                }
            }
        });
        if let Some(collapsed) = collapse_all
            && let Some(entry) = self
                .top_level_entries
                .iter()
                .find(|entry| entry.id.id == self.selected_top_tab_id)
        {
            let mut paths = vec![];
            entry.control.collect_collapsible(&mut paths);
            path_value_pairs.extend(paths.into_iter().map(|mut path| {
                path.push("gui_collapsed".into());
                PathValuePair {
                    path,
                    value: json::Value::Bool(collapsed),
                }
            }));
        }

        set_filter(ui, query.clone());

        ScrollArea::vertical()
            .id_salt(format!("{}_scroll", self.selected_top_tab_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(MAX_CONTENT_WIDTH);

                if let Some(query) = &query {
                    self.search_results_ui(ui, query, &mut path_value_pairs);
                } else if self.selected_top_tab_id == "presets" {
                    self.presets_ui(ui, &mut path_value_pairs);
                } else {
                    self.category_ui(ui, &mut path_value_pairs);
                }
            });

        set_filter(ui, None);

        path_value_pairs
    }

    fn presets_ui(&mut self, ui: &mut Ui, path_value_pairs: &mut Vec<PathValuePair>) {
        Grid::new("presets_grid")
            .striped(true)
            .num_columns(2)
            .min_col_width(MIN_COLUMN_SIZE)
            .show(ui, |ui| {
                for preset in [
                    &mut self.resolution_preset,
                    &mut self.framerate_preset,
                    &mut self.encoder_preset,
                    &mut self.foveation_preset,
                    &mut self.codec_preset,
                    &mut self.game_audio_preset,
                    &mut self.microphone_preset,
                    &mut self.hand_tracking_interaction_preset,
                    &mut self.eye_face_tracking_preset,
                ] {
                    path_value_pairs.extend(preset.ui(ui));
                    ui.end_row();
                }
            });
    }

    fn category_ui(&mut self, ui: &mut Ui, path_value_pairs: &mut Vec<PathValuePair>) {
        let Some(session_fragment) = &mut self.session_settings_json else {
            return;
        };
        let Some(entry) = self
            .top_level_entries
            .iter_mut()
            .find(|entry| entry.id.id == self.selected_top_tab_id)
        else {
            return;
        };

        Grid::new(format!("{}_grid", self.selected_top_tab_id))
            .striped(true)
            .num_columns(2)
            .min_col_width(MIN_COLUMN_SIZE)
            .show(ui, |ui| {
                path_value_pairs.extend(entry.control.ui(
                    ui,
                    &mut session_fragment.as_object_mut().unwrap()[&entry.id.id],
                    false,
                ));
                ui.end_row();
            });
    }

    // Every category with at least one matching setting, with only the matches shown
    fn search_results_ui(
        &mut self,
        ui: &mut Ui,
        query: &str,
        path_value_pairs: &mut Vec<PathValuePair>,
    ) {
        let Some(session_fragment) = &mut self.session_settings_json else {
            return;
        };

        let mut found = false;
        for entry in &mut self.top_level_entries {
            if !entry.control.contains_text(query) {
                continue;
            }
            found = true;

            ui.add_space(6.0);
            ui.label(RichText::new(&entry.id.display).size(18.0).strong());
            Grid::new(format!("{}_search_grid", entry.id.id))
                .striped(true)
                .num_columns(2)
                .min_col_width(MIN_COLUMN_SIZE)
                .show(ui, |ui| {
                    path_value_pairs.extend(entry.control.ui(
                        ui,
                        &mut session_fragment.as_object_mut().unwrap()[&entry.id.id],
                        false,
                    ));
                    ui.end_row();
                });
        }

        if !found {
            ui.label(tr("No matching settings"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alvr_packets::PathSegment;

    // "Expand all" writes `gui_collapsed` below each of these paths: they must exist in the session
    #[test]
    fn collapsible_paths_exist_in_session() {
        let tab = SettingsTab::new();
        let session = json::to_value(alvr_session::SessionConfig::default().session_settings).unwrap();

        let mut found = 0;
        for entry in &tab.top_level_entries {
            let mut paths = vec![];
            entry.control.collect_collapsible(&mut paths);

            for path in paths {
                // The first segment is "session_settings", the root of `session`
                let mut value = &session;
                for segment in &path[1..] {
                    let PathSegment::Name(name) = segment else {
                        panic!("unexpected index in {path:?}");
                    };
                    value = value.get(name).unwrap_or_else(|| panic!("missing {path:?}"));
                }
                assert!(value.get("gui_collapsed").is_some(), "no gui_collapsed at {path:?}");
                found += 1;
            }
        }
        assert!(found > 0);
    }

    #[test]
    fn search_finds_settings_by_either_language() {
        let tab = SettingsTab::new();
        let video = tab.top_level_entries.iter().find(|e| e.id.id == "video").unwrap();

        assert!(video.control.contains_text("bitrate"));
        assert!(video.control.contains_text("ビットレート"));
        assert!(!video.control.contains_text("zzzz"));
    }
}
