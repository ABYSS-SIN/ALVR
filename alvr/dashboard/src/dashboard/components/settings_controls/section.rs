use alvr_gui_common::i18n::tr;
use super::{
    INDENTATION_STEP, MATCH_ALL, NestingInfo, SettingControl, active_filter, collapsible, notice,
    set_filter, text_matches,
};
use alvr_gui_common::{
    DisplayString,
    theme::{
        OK_GREEN,
        log_colors::{INFO_LIGHT, WARNING_LIGHT},
    },
};
use alvr_packets::{PathSegment, PathValuePair};
use alvr_session::settings_schema::{SchemaEntry, SchemaNode};
use eframe::egui::Ui;
use serde_json as json;

struct Entry {
    id: DisplayString,
    help: Option<String>,
    notice: Option<String>,
    hidden: bool,
    steamvr_restart_flag: bool,
    real_time_flag: bool,
    control: SettingControl,
}

impl Entry {
    // The search also accepts the English identifier, as the documentation names settings that way
    fn name_matches(&self, query: &str) -> bool {
        text_matches(&self.id.display, query) || text_matches(&self.id.id.replace('_', " "), query)
    }

    fn matches(&self, query: &str) -> bool {
        self.name_matches(query) || self.control.contains_text(query)
    }

    fn is_collapsible(&self) -> bool {
        matches!(&self.control, SettingControl::Section(control) if control.gui_collapsible)
    }
}

pub struct Control {
    nesting_info: NestingInfo,
    entries: Vec<Entry>,
    gui_collapsible: bool,
}

impl Control {
    pub fn new(
        mut nesting_info: NestingInfo,
        schema_entries: Vec<SchemaEntry<SchemaNode>>,
        gui_collapsible: bool,
    ) -> Self {
        nesting_info.indentation_level += 1;

        let entries = schema_entries
            .into_iter()
            .map(|entry| {
                let id = entry.name;
                let display = super::get_display_name(&id, &entry.strings);
                let help = entry.strings.get("help").map(|s| tr(s));
                let notice = entry.strings.get("notice").map(|s| tr(s));
                let hidden = entry.flags.contains("hidden");
                let steamvr_restart_flag = entry.flags.contains("steamvr-restart");
                let real_time_flag = entry.flags.contains("real-time");

                let mut nesting_info = nesting_info.clone();
                nesting_info.path.push(id.clone().into());

                Entry {
                    id: DisplayString { id, display },
                    help,
                    notice,
                    hidden,
                    steamvr_restart_flag,
                    real_time_flag,
                    control: SettingControl::new(nesting_info, entry.content),
                }
            })
            .collect();

        Self {
            nesting_info,
            entries,
            gui_collapsible,
        }
    }

    pub fn contains_text(&self, query: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| !entry.hidden && entry.matches(query))
    }

    pub fn collect_collapsible(&self, out: &mut Vec<Vec<PathSegment>>) {
        if self.gui_collapsible {
            out.push(self.nesting_info.path.clone());
        }
        for entry in self.entries.iter().filter(|entry| !entry.hidden) {
            entry.control.collect_collapsible(out);
        }
    }

    fn collapsed_request(&self, collapsed: bool) -> Option<PathValuePair> {
        super::get_single_value(
            &self.nesting_info,
            "gui_collapsed".into(),
            json::Value::Bool(collapsed),
        )
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        session_fragment: &mut json::Value,
        allow_inline: bool,
    ) -> Option<PathValuePair> {
        let filter = active_filter(ui);
        let mut request = None;

        // A search shows every match, whatever the sections' state is
        let collapsed = if self.gui_collapsible && filter.is_none() {
            let collapsed = collapsible::is_collapsed(session_fragment);

            // When the parent already drew the name, it also draws the header. Otherwise the label
            // cell is free and the header goes there.
            if !allow_inline {
                let level = self.nesting_info.indentation_level.saturating_sub(1);
                let clicked = ui
                    .horizontal(|ui| {
                        ui.add_space(INDENTATION_STEP * level as f32);
                        collapsible::header(ui, &tr("Details"), collapsed).clicked()
                    })
                    .inner;

                if clicked {
                    collapsible::set_collapsed(session_fragment, !collapsed);
                    request = self.collapsed_request(!collapsed);
                }
            }

            if !collapsed {
                ui.end_row();
            }

            collapsed
        } else {
            if allow_inline {
                ui.end_row();
            }

            false
        };

        if collapsed {
            return request;
        }

        let visible: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let entry = &self.entries[i];
                !entry.hidden && filter.as_ref().is_none_or(|query| entry.matches(query))
            })
            .collect();

        for (position, &i) in visible.iter().enumerate() {
            let entry = &mut self.entries[i];
            let fragment = &mut session_fragment[&entry.id.id];

            // What is under an entry that matched by name is shown whole
            let matched_by_name = filter.as_ref().is_some_and(|query| entry.name_matches(query));

            if let Some(string) = &entry.notice {
                ui.label(tr(" "));
                notice::notice(ui, string);
                ui.end_row();
            }

            let own_header = entry.is_collapsible() && filter.is_none();
            let mut toggled_to = None;
            ui.horizontal(|ui| {
                ui.add_space(INDENTATION_STEP * self.nesting_info.indentation_level as f32);

                let label_res = if own_header {
                    let collapsed = collapsible::is_collapsed(fragment);
                    let response = collapsible::header(ui, &entry.id.display, collapsed);
                    if response.clicked() {
                        toggled_to = Some(!collapsed);
                    }
                    response
                } else {
                    ui.label(&entry.id.display)
                };
                if cfg!(debug_assertions) {
                    label_res.on_hover_text_at_pointer(&*entry.id);
                }

                if let Some(string) = &entry.help {
                    ui.colored_label(INFO_LIGHT, "❓")
                        .on_hover_text_at_pointer(string);
                }
                if entry.steamvr_restart_flag {
                    ui.colored_label(WARNING_LIGHT, "⚠")
                        .on_hover_text_at_pointer(
                            tr("Changing this setting will make SteamVR restart!\n\
                            Please save your in-game progress first"),
                        );
                }
                if entry.real_time_flag {
                    // The emoji is blue but it will be green in the UI
                    ui.colored_label(OK_GREEN, "🔵").on_hover_text_at_pointer(
                        tr("This setting can be changed in real-time during streaming!"),
                    );
                }
            });

            if let Some(collapsed) = toggled_to {
                collapsible::set_collapsed(fragment, collapsed);

                let mut path = self.nesting_info.path.clone();
                path.push(entry.id.id.clone().into());
                path.push("gui_collapsed".into());
                request = Some(PathValuePair {
                    path,
                    value: json::Value::Bool(collapsed),
                });
            }

            if matched_by_name {
                set_filter(ui, Some(MATCH_ALL.into()));
            }
            request = entry.control.ui(ui, fragment, true).or(request);
            if matched_by_name {
                set_filter(ui, filter.clone());
            }

            if position + 1 != visible.len() {
                ui.end_row();
            }
        }

        request
    }
}
