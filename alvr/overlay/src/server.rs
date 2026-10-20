//! Link with the ALVR server: same HTTP/WebSocket API the dashboard uses when the server is running.

use alvr_events::{Event, EventType, StatisticsSummary};
use alvr_packets::{PathValuePair, parse_path};
use serde_json as json;
use std::{
    io::ErrorKind,
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    client::IntoClientRequest,
    http::{HeaderValue, Uri},
};

const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct State {
    pub connected: bool,
    // Session as JSON: settings are read and written by path, no need for the typed struct
    pub session: Option<json::Value>,
    pub stats: Option<(Instant, StatisticsSummary)>,
}

pub enum Command {
    SetValue { path: String, value: json::Value },
    InsertIdr,
}

pub struct Server {
    pub state: Arc<Mutex<State>>,
    commands: mpsc::Sender<Command>,
}

impl Server {
    pub fn spawn(port: u16) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (commands, commands_receiver) = mpsc::channel();

        thread::spawn({
            let state = Arc::clone(&state);
            move || events_loop(port, &state)
        });
        thread::spawn(move || commands_loop(port, commands_receiver));

        Self { state, commands }
    }

    pub fn send(&self, command: Command) {
        self.commands.send(command).ok();
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into()
}

fn events_loop(port: u16, state: &Mutex<State>) {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let agent = agent();

    loop {
        let Ok(socket) = TcpStream::connect_timeout(&address, REQUEST_TIMEOUT) else {
            thread::sleep(Duration::from_secs(1));
            continue;
        };

        let uri = Uri::try_from(format!("ws://127.0.0.1:{port}/api/events")).unwrap();
        let mut request = uri.into_client_request().unwrap();
        request
            .headers_mut()
            .insert("X-ALVR", HeaderValue::from_static("true"));

        let Ok((mut ws, _)) = tungstenite::client(request, socket) else {
            thread::sleep(Duration::from_secs(1));
            continue;
        };
        ws.get_mut().set_nonblocking(true).ok();

        state.lock().unwrap().connected = true;

        // The server answers with a Session event
        agent
            .get(format!("http://127.0.0.1:{port}/api/session"))
            .header("X-ALVR", "true")
            .call()
            .ok();

        loop {
            match ws.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    let Ok(event) = json::from_str::<Event>(&text) else {
                        continue;
                    };

                    let mut state = state.lock().unwrap();
                    match event.event_type {
                        EventType::Session(session) => {
                            state.session = json::to_value(*session).ok();
                        }
                        EventType::StatisticsSummary(stats) => {
                            state.stats = Some((Instant::now(), stats));
                        }
                        _ => (),
                    }
                }
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
                _ => (),
            }
        }

        let mut state = state.lock().unwrap();
        state.connected = false;
        state.stats = None;
    }
}

fn commands_loop(port: u16, commands: mpsc::Receiver<Command>) {
    let agent = agent();
    let base = format!("http://127.0.0.1:{port}/api");

    while let Ok(command) = commands.recv() {
        match command {
            Command::SetValue { path, value } => {
                let Ok(path) = parse_path(&path) else {
                    continue;
                };
                agent
                    .post(format!("{base}/session/values"))
                    .header("X-ALVR", "true")
                    .send_json(vec![PathValuePair { path, value }])
                    .ok();
            }
            Command::InsertIdr => {
                agent
                    .post(format!("{base}/insert-idr"))
                    .header("X-ALVR", "true")
                    .send_empty()
                    .ok();
            }
        }
    }
}
