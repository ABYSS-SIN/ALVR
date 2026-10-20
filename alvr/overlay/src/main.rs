//! ALVR settings panel inside the SteamVR dashboard.
//!
//! Runs as its own process (an OpenVR overlay app) and talks to the ALVR server through its web
//! API, so it works alongside the regular dashboard. Start it before or after SteamVR: it waits.

// hide the console window on Windows
#![cfg_attr(windows, windows_subsystem = "windows")]

mod openvr;
mod openvr_tables;
mod render;
mod server;
mod ui;

use anyhow::Result;
use egui::{Event, PointerButton, Pos2, Vec2};
use openvr::{MOUSE_BUTTON_LEFT, OpenVr, OverlayEvent};
use render::Gui;
use server::{Server, State};
use std::{
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

const TEXTURE_SIZE: [u32; 2] = [1280, 1152];
const PIXELS_PER_POINT: f32 = 1.4;
const OVERLAY_WIDTH_METERS: f32 = 2.0;
const DEFAULT_SERVER_PORT: u16 = 8082;
// Only used as a "single instance" lock
const LOCK_PORT: u16 = 8099;
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

fn to_egui_events(events: Vec<OverlayEvent>) -> (Vec<Event>, bool) {
    let mut out = vec![];
    let mut quit = false;

    for event in events {
        let point = |x: f32, y: f32| Pos2::new(x, y) / PIXELS_PER_POINT;

        match event {
            OverlayEvent::MouseMove { x, y } => out.push(Event::PointerMoved(point(x, y))),
            OverlayEvent::MouseButton { x, y, button, pressed } => {
                let button = if button == MOUSE_BUTTON_LEFT {
                    PointerButton::Primary
                } else {
                    PointerButton::Secondary
                };
                // Make sure the pointer is where the click happens
                out.push(Event::PointerMoved(point(x, y)));
                out.push(Event::PointerButton {
                    pos: point(x, y),
                    button,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            OverlayEvent::Scroll { x, y } => out.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: Vec2::new(x, y),
                modifiers: Default::default(),
                phase: egui::TouchPhase::Move,
            }),
            OverlayEvent::FocusLeave => out.push(Event::PointerGone),
            OverlayEvent::Quit => quit = true,
        }
    }

    (out, quit)
}

fn run_overlay(gui: &mut Gui, openvr: &OpenVr, server: &Server) {
    let thumbnail = [128, 128];
    openvr.set_thumbnail(&[0x30, 0x6c, 0xd8, 0xff].repeat(128 * 128), thumbnail);

    loop {
        let frame_start = Instant::now();

        let (events, quit) = to_egui_events(openvr.poll_events());
        if quit {
            return;
        }

        // Nothing to draw while the dashboard is closed
        if openvr.is_dashboard_visible() {
            let mut commands = vec![];
            let pixels = {
                let state = server.state.lock().unwrap();
                gui.frame(events, |ui| commands = ui::panel(ui, &state))
            };
            openvr.set_texture(pixels);

            for command in commands {
                server.send(command);
            }
        } else {
            thread::sleep(Duration::from_millis(100));
        }

        thread::sleep(FRAME_INTERVAL.saturating_sub(frame_start.elapsed()));
    }
}

// Renders the panel with fake data to a PNG, to check the layout without SteamVR
fn snapshot(path: &str) -> Result<()> {
    let session = serde_json::to_value(alvr_session::SessionConfig::default())?;
    let state = State {
        connected: true,
        session: Some(session),
        stats: Some((
            Instant::now(),
            alvr_events::StatisticsSummary {
                video_mbits_per_sec: 96.4,
                video_packet_loss_percent: 0.35,
                total_latency_ms: 38.0,
                network_latency_ms: 6.2,
                encode_latency_ms: 4.1,
                decode_latency_ms: 3.3,
                client_fps: 90,
                server_fps: 90,
                ..Default::default()
            },
        )),
    };

    let mut gui = Gui::new(TEXTURE_SIZE, PIXELS_PER_POINT)?;
    // The first frame only loads fonts and textures
    gui.frame(vec![], |ui| _ = ui::panel(ui, &state));
    let pixels = gui.frame(vec![], |ui| _ = ui::panel(ui, &state)).to_vec();

    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, TEXTURE_SIZE[0], TEXTURE_SIZE[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;

    Ok(())
}

// Connects to SteamVR once and shows a blank panel, to check the OpenVR side without a headset
fn probe() -> Result<()> {
    let mut gui = Gui::new(TEXTURE_SIZE, PIXELS_PER_POINT)?;
    let openvr = OpenVr::connect(gui.size(), OVERLAY_WIDTH_METERS)?;
    println!("connected, dashboard visible: {}", openvr.is_dashboard_visible());

    let pixels = gui.frame(vec![], |ui| _ = ui.label("probe")).to_vec();
    openvr.set_texture(&pixels);
    println!("texture set, {} events", openvr.poll_events().len());

    Ok(())
}

// Reads a setting from the running server, changes it, checks the change comes back, restores it
fn probe_server() -> Result<()> {
    const PATH: &str = "session_settings.video.preferred_fps";

    let server = Server::spawn(DEFAULT_SERVER_PORT);
    let read = || {
        let state = server.state.lock().unwrap();
        let fps = state.session.as_ref().and_then(|s| {
            PATH.split('.')
                .try_fold(s, |v, key| v.get(key))
                .and_then(|v| v.as_f64())
        });
        (state.connected, fps)
    };
    let wait_for = |expected: Option<f64>| {
        (0..50).any(|_| {
            thread::sleep(Duration::from_millis(100));
            read().1 == expected
        })
    };

    for _ in 0..50 {
        if read().1.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    anyhow::ensure!(read().1.is_some(), "no session received");
    let original = read().1;
    println!("connected: {}, {PATH} = {original:?}", read().0);

    let changed = if original == Some(90.0) { 80.0 } else { 90.0 };
    server.send(server::Command::SetValue { path: PATH.into(), value: changed.into() });
    println!("changed to {changed}: {}", wait_for(Some(changed)));

    server.send(server::Command::SetValue { path: PATH.into(), value: original.into() });
    println!("restored: {}", wait_for(original));

    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--probe-server") {
        return probe_server();
    }
    if args.iter().any(|a| a == "--probe") {
        return probe();
    }
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        return snapshot(args.get(i + 1).map_or("overlay.png", |s| s));
    }

    let Ok(_lock) = TcpListener::bind(("127.0.0.1", LOCK_PORT)) else {
        return Ok(()); // already running
    };

    let port = std::env::var("ALVR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_SERVER_PORT);
    let server = Server::spawn(port);
    let mut gui = Gui::new(TEXTURE_SIZE, PIXELS_PER_POINT)?;

    // SteamVR may not be running yet, and it can be restarted while this runs (ALVR does it to
    // apply some settings): wait for it again each time instead of exiting.
    loop {
        match OpenVr::connect(gui.size(), OVERLAY_WIDTH_METERS) {
            Ok(openvr) => run_overlay(&mut gui, &openvr, &server),
            Err(_) => thread::sleep(Duration::from_secs(2)),
        }
    }
}
