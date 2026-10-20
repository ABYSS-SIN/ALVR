use alvr_gui_common::i18n::tr;
use crate::dashboard::ServerRequest;
use eframe::{
    egui::{Button, Label, Layout, RichText, Ui},
    emath::Align,
};

pub enum SetupWizardRequest {
    ServerRequest(ServerRequest),
    Close { finished: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Welcome = 0,
    HardwareRequirements = 1,
    SoftwareRequirements = 2,
    Firewall = 3,
    Recommendations = 4,
    Finished = 5,
}

fn index_to_page(index: usize) -> Page {
    match index {
        0 => Page::Welcome,
        1 => Page::HardwareRequirements,
        2 => Page::SoftwareRequirements,
        3 => Page::Firewall,
        4 => Page::Recommendations,
        5 => Page::Finished,
        _ => panic!("Invalid page index"),
    }
}

fn page_content(
    ui: &mut Ui,
    subtitle: &str,
    paragraph: &str,
    interactible_content: impl FnMut(&mut Ui),
) {
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        ui.add_space(60.0);
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add_space(60.0);
            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                ui.add_space(15.0);
                ui.heading(RichText::new(subtitle).size(20.0));
                ui.add(Label::new(RichText::new(tr(paragraph)).size(14.0)).wrap());
                ui.add_space(30.0);
                ui.vertical_centered(interactible_content);
            });
        })
    });
}

pub struct SetupWizard {
    page: Page,
}

impl SetupWizard {
    pub fn new() -> Self {
        Self {
            page: Page::Welcome,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<SetupWizardRequest> {
        let mut request = None;

        ui.horizontal(|ui| {
            ui.add_space(60.0);
            ui.vertical(|ui| {
                ui.add_space(30.0);
                ui.heading(RichText::new(tr("Welcome to ALVR")).size(30.0));
                ui.add_space(5.0);
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(15.0);
                if ui.button(tr("❌")).clicked() {
                    request = Some(SetupWizardRequest::Close { finished: false });
                }
            })
        });
        ui.separator();
        match &self.page {
            Page::Welcome => page_content(
                ui,
                &tr("This setup wizard will help you setup ALVR."),
                "",
                |_| (),
            ),
            Page::HardwareRequirements => page_content(
                ui,
                &tr("Hardware requirements"),
                r"ALVR requires a dedicated and recent graphics card. Low-end Intel integrated graphics may fail to work.
Make sure you have at least one output audio device.",
                |_| (),
            ),
            Page::SoftwareRequirements => page_content(
                ui,
                &tr("Software requirements"),
                if cfg!(windows) {
                    r"To stream the headset microphone on Windows you need to install Virtual Audio Cable, VB-Cable, Voicemeeter"
                } else if cfg!(target_os = "linux") {
                    r"You need the PipeWire (0.3.49+ version) audio system to be able to stream audio and use microphone."
                } else {
                    r"Unsupported OS"
                },
                #[allow(unused_variables)]
                |ui| {
                    #[cfg(windows)]
                    if ui.button(tr("Download Virtual Audio Cable (Lite)")).clicked() {
                        ui.ctx().open_url(eframe::egui::OpenUrl::same_tab(
                            "https://software.muzychenko.net/freeware/vac470lite.zip",
                        ));
                    }
                },
            ),
            Page::Firewall => page_content(
                ui,
                &tr("Firewall"),
                r"To communicate with the headset, some firewall rules need to be set.
This requires administrator rights!",
                |ui| {
                    if ui.button(tr("Add firewall rules")).clicked() {
                        request = Some(SetupWizardRequest::ServerRequest(
                            ServerRequest::AddFirewallRules,
                        ));
                    }
                },
            ),
            Page::Recommendations => page_content(
                ui,
                &tr("Recommendations"),
                r"ALVR supports multiple types of PC hardware and headsets but not all might work correctly with default settings. Please try tweaking different settings like resolution, bitrate, encoder and others if your ALVR experience is not optimal.",
                |_| (),
            ),
            Page::Finished => page_content(
                ui,
                &tr("Finished"),
                r#"You can always restart this setup wizard from the "Installation" tab on the left."#,
                |_| (),
            ),
        };

        ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                ui.add_space(15.0);
                if self.page == Page::Finished {
                    if ui.button(tr("Finish")).clicked() {
                        request = Some(SetupWizardRequest::Close { finished: true });
                    }
                } else if ui.button(tr("Next")).clicked() {
                    self.page = index_to_page(self.page as usize + 1);
                }
                if ui
                    .add_visible(self.page != Page::Welcome, Button::new(tr("Back")))
                    .clicked()
                {
                    self.page = index_to_page(self.page as usize - 1);
                }
            });
            ui.separator();
        });

        request
    }
}
