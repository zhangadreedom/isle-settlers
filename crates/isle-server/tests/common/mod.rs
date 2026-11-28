//! Shared harness for the server integration tests: a server on a random
//! port, a WebSocket protocol client and a tiny HTTP client.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use isle_core::{Action, Event};
use isle_proto::PlayerView;
use isle_proto::net::{BotLevel, ChatLine, ClientMsg, LegalActions, RoomInfo, ServerMsg};
use isle_server::{AppState, ServerConfig, app};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub fn fast_config() -> ServerConfig {
    ServerConfig {
        bot_delay_scale: 0.0,
        autopilot_after: Duration::from_secs(3600),
        ..ServerConfig::default()
    }
}

pub struct Server {
    pub addr: SocketAddr,
    pub state: AppState,
    task: JoinHandle<()>,
}

impl Server {
    pub async fn start(config: ServerConfig) -> Server {
        let state = AppState::new(config).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = app(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
        let task = tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        });
        Server { addr, state, task }
    }

    pub async fn stop(self) {
        self.state.registry.shutdown().await;
        self.task.abort();
    }
}

/// A protocol client that keeps the latest state it was sent.
pub struct Client {
    pub ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    pub room_id: String,
    pub seat: Option<u8>,
    pub token: Option<String>,
    pub room: Option<RoomInfo>,
    pub view: Option<PlayerView>,
    pub legal: Option<LegalActions>,
    pub events: Vec<Event>,
    pub errors: Vec<String>,
    pub chats: Vec<ChatLine>,
}

impl Client {
    pub async fn connect(addr: SocketAddr) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/ws"))
            .await
            .unwrap();
        Client {
            ws,
            room_id: String::new(),
            seat: None,
            token: None,
            room: None,
            view: None,
            legal: None,
            events: Vec::new(),
            errors: Vec::new(),
            chats: Vec::new(),
        }
    }

    pub async fn create(addr: SocketAddr, players: u8, name: &str) -> Client {
        Client::create_as(addr, players, name, None).await
    }

    /// Creates a room, signed in with `session` if given.
    pub async fn create_as(
        addr: SocketAddr,
        players: u8,
        name: &str,
        session: Option<&str>,
    ) -> Client {
        Client::create_room(addr, players, name, session, false).await
    }

    /// Creates a ranked or casual room.
    pub async fn create_room(
        addr: SocketAddr,
        players: u8,
        name: &str,
        session: Option<&str>,
        ranked: bool,
    ) -> Client {
        let mut c = Client::connect(addr).await;
        c.send(&ClientMsg::Create {
            players,
            bots: BotLevel::Normal,
            name: name.into(),
            ranked,
            scenario: Default::default(),
            rules: Default::default(),
            session: session.map(str::to_owned),
            is_public: false,
        })
        .await;
        c.until(|c| c.room.is_some() || !c.errors.is_empty()).await;
        c
    }

    pub async fn join(addr: SocketAddr, room: &str, name: &str, token: Option<String>) -> Client {
        Client::join_as(addr, room, name, token, None).await
    }

    /// Joins a room, signed in with `session` if given.
    pub async fn join_as(
        addr: SocketAddr,
        room: &str,
        name: &str,
        token: Option<String>,
        session: Option<&str>,
    ) -> Client {
        let mut c = Client::connect(addr).await;
        c.send(&ClientMsg::Join {
            room: room.into(),
            name: name.into(),
            token,
            session: session.map(str::to_owned),
        })
        .await;
        c.until(|c| c.room.is_some() || !c.errors.is_empty()).await;
        c
    }

    pub async fn send(&mut self, msg: &ClientMsg) {
        self.ws
            .send(Message::Text(serde_json::to_string(msg).unwrap().into()))
            .await
            .unwrap();
    }

    /// Receives one message (5 s timeout) and folds it into the state.
    pub async fn recv(&mut self) -> ServerMsg {
        loop {
            let frame = timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("timed out waiting for the server")
                .expect("socket closed")
                .unwrap();
            let Message::Text(text) = frame else { continue };
            let msg: ServerMsg = serde_json::from_str(&text).unwrap();
            match &msg {
                ServerMsg::Joined { room, seat, token } => {
                    self.room_id = room.clone();
                    self.seat = *seat;
                    self.token = token.clone();
                }
                ServerMsg::Room(r) => self.room = Some(r.clone()),
                ServerMsg::Snapshot(s) => {
                    self.view = Some(s.view.clone());
                    self.legal = Some(s.legal.clone());
                }
                ServerMsg::Update(u) => {
                    self.view = Some(u.view.clone());
                    self.legal = Some(u.legal.clone());
                    self.events.extend(u.events.iter().copied());
                }
                ServerMsg::Chat(c) => self.chats.push(c.clone()),
                ServerMsg::Error { code } => self.errors.push(code.clone()),
            }
            return msg;
        }
    }

    pub async fn until(&mut self, done: impl Fn(&Client) -> bool) {
        while !done(self) {
            self.recv().await;
        }
    }

    pub fn game_over(&self) -> bool {
        self.view.as_ref().is_some_and(|v| v.winner.is_some())
    }
}

/// Simple progress-making policy over the legal actions.
pub fn choose(legal: &[Action]) -> Option<Action> {
    let rank = |a: &Action| match a {
        Action::PlaceCamel(_) => 0,
        Action::BuildCity(_) => 1,
        Action::BuildSettlement(_) => 2,
        Action::BuildBridge(_) | Action::BuildRoad(_) => 3,
        Action::RollDice => 4,
        Action::Discard(_) | Action::MoveRobber { .. } => 5,
        Action::RespondTrade { accept: false } => 6,
        Action::EndTurn => 7,
        _ => 9,
    };
    legal.iter().copied().min_by_key(rank)
}

/// Plays whatever this client is asked to until the game ends.
pub async fn play_to_end(mut c: Client) -> Client {
    let mut acted_at = None;
    while !c.game_over() {
        if let Some(legal) = c.legal.clone()
            && acted_at != Some(legal.version)
            && let Some(action) = choose(&legal.actions)
        {
            acted_at = Some(legal.version);
            c.send(&ClientMsg::Act {
                version: legal.version,
                action,
            })
            .await;
        }
        c.recv().await;
    }
    c
}

/// A plain HTTP/1.1 request; returns the status code, the head and the body.
pub async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String, String) {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(body) = body {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    req.push_str("\r\n");
    req.push_str(body.unwrap_or(""));
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    tcp.write_all(req.as_bytes()).await.unwrap();
    let mut raw = String::new();
    tcp.read_to_string(&mut raw).await.unwrap();
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head.to_owned(), body.to_owned())
}

/// `GET path`; returns the status line and the body.
pub async fn http_get(addr: SocketAddr, path: &str) -> (String, String) {
    let (_, head, body) = http(addr, "GET", path, &[], None).await;
    (head.lines().next().unwrap().to_owned(), body)
}

/// A fresh, empty directory under the system temp dir.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("isle-server-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}
