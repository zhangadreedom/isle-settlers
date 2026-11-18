//! End-to-end tests over real WebSockets against a server on a random port.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use isle_core::{Action, Event, Phase};
use isle_proto::PlayerView;
use isle_proto::net::{BotLevel, ChatLine, ClientMsg, LegalActions, RoomInfo, SeatView, ServerMsg};
use isle_server::{AppState, ServerConfig, app};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

fn fast_config() -> ServerConfig {
    ServerConfig {
        bot_delay_scale: 0.0,
        autopilot_after: Duration::from_secs(3600),
        ..ServerConfig::default()
    }
}

struct Server {
    addr: SocketAddr,
    state: AppState,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(config: ServerConfig) -> Server {
        let state = AppState::new(config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = app(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Server { addr, state, task }
    }

    async fn stop(self) {
        self.state.registry.shutdown().await;
        self.task.abort();
    }
}

/// A protocol client that keeps the latest state it was sent.
struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    room_id: String,
    seat: Option<u8>,
    token: Option<String>,
    room: Option<RoomInfo>,
    view: Option<PlayerView>,
    legal: Option<LegalActions>,
    events: Vec<Event>,
    errors: Vec<String>,
    chats: Vec<ChatLine>,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Client {
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

    async fn create(addr: SocketAddr, players: u8, name: &str) -> Client {
        let mut c = Client::connect(addr).await;
        c.send(&ClientMsg::Create {
            players,
            bots: BotLevel::Normal,
            name: name.into(),
        })
        .await;
        c.until(|c| c.room.is_some()).await;
        c
    }

    async fn join(addr: SocketAddr, room: &str, name: &str, token: Option<String>) -> Client {
        let mut c = Client::connect(addr).await;
        c.send(&ClientMsg::Join {
            room: room.into(),
            name: name.into(),
            token,
        })
        .await;
        c.until(|c| c.room.is_some() || !c.errors.is_empty()).await;
        c
    }

    async fn send(&mut self, msg: &ClientMsg) {
        self.ws
            .send(Message::Text(serde_json::to_string(msg).unwrap().into()))
            .await
            .unwrap();
    }

    /// Receives one message (5 s timeout) and folds it into the state.
    async fn recv(&mut self) -> ServerMsg {
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

    async fn until(&mut self, done: impl Fn(&Client) -> bool) {
        while !done(self) {
            self.recv().await;
        }
    }

    fn game_over(&self) -> bool {
        self.view.as_ref().is_some_and(|v| v.winner.is_some())
    }
}

/// Simple progress-making policy over the legal actions.
fn choose(legal: &[Action]) -> Option<Action> {
    let rank = |a: &Action| match a {
        Action::BuildCity(_) => 0,
        Action::BuildSettlement(_) => 1,
        Action::BuildRoad(_) => 2,
        Action::RollDice => 3,
        Action::Discard(_) | Action::MoveRobber { .. } => 4,
        Action::RespondTrade { accept: false } => 5,
        Action::EndTurn => 6,
        _ => 9,
    };
    legal.iter().copied().min_by_key(rank)
}

/// Plays whatever this client is asked to until the game ends.
async fn play_to_end(mut c: Client) -> Client {
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

#[tokio::test]
async fn two_humans_and_bots_play_a_full_game() {
    let server = Server::start(fast_config()).await;
    let mut host = Client::create(server.addr, 4, "Alice").await;
    assert_eq!(host.seat, Some(0));
    assert!(host.token.as_ref().is_some_and(|t| t.len() == 32));

    let mut guest = Client::join(server.addr, &host.room_id.to_lowercase(), "Bob", None).await;
    assert_eq!(guest.seat, Some(1));
    host.until(|c| matches!(c.room.as_ref().unwrap().seats[1], SeatView::Human { .. }))
        .await;

    host.send(&ClientMsg::SetSeat {
        seat: 3,
        bot: Some(BotLevel::Easy),
    })
    .await;
    host.until(|c| {
        c.room.as_ref().unwrap().seats[3]
            == SeatView::Bot {
                level: BotLevel::Easy,
            }
    })
    .await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    guest.until(|c| c.view.is_some()).await;

    let room = host.room.clone().unwrap();
    assert!(room.started);
    assert_eq!(
        room.seats[2],
        SeatView::Bot {
            level: BotLevel::Normal
        },
        "open seat filled on start"
    );

    let (host, guest) = tokio::join!(
        timeout(Duration::from_secs(60), play_to_end(host)),
        timeout(Duration::from_secs(60), play_to_end(guest))
    );
    let (host, guest) = (
        host.expect("host timed out"),
        guest.expect("guest timed out"),
    );
    let (hv, gv) = (host.view.unwrap(), guest.view.unwrap());
    assert_eq!(hv.winner, gv.winner);
    assert_eq!(hv.me.unwrap().seat, 0);
    assert_eq!(gv.me.unwrap().seat, 1);
    assert!(hv.final_points.is_some());

    // Private details of other seats never reach a client.
    for (client, seat) in [(&host.events, 0u8), (&guest.events, 1u8)] {
        for e in client {
            match e {
                Event::DevCardBought { player, card } if *player != seat => assert!(card.is_none()),
                Event::Stolen {
                    thief,
                    victim,
                    resource,
                } if *thief != seat && *victim != seat => {
                    assert!(resource.is_none())
                }
                _ => {}
            }
        }
    }
    server.stop().await;
}

#[tokio::test]
async fn reconnecting_with_the_token_restores_the_seat() {
    let server = Server::start(fast_config()).await;
    let mut host = Client::create(server.addr, 2, "Alice").await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    // Place the first settlement and road, wait for the bot's two
    // placements, then drop the connection while it is our turn again.
    for _ in 0..2 {
        let legal = host.legal.clone().unwrap();
        host.send(&ClientMsg::Act {
            version: legal.version,
            action: legal.actions[0],
        })
        .await;
        host.until(|c| c.legal.as_ref().unwrap().version > legal.version)
            .await;
    }
    host.until(|c| !c.legal.as_ref().unwrap().actions.is_empty())
        .await;
    let (room, token, before) = (
        host.room_id.clone(),
        host.token.clone(),
        host.view.clone().unwrap(),
    );
    drop(host);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut back = Client::join(server.addr, &room, "Someone else", token).await;
    back.until(|c| c.view.is_some()).await;
    assert_eq!(back.seat, Some(0));
    let SeatView::Human {
        name, connected, ..
    } = &back.room.as_ref().unwrap().seats[0]
    else {
        panic!()
    };
    assert_eq!(name, "Alice", "a rejoining player keeps their name");
    assert!(connected);
    // Bots pause while no human is connected, so nothing changed meanwhile.
    assert_eq!(back.view.unwrap(), before);
    server.stop().await;
}

#[tokio::test]
async fn autopilot_plays_for_a_disconnected_player() {
    let config = ServerConfig {
        autopilot_after: Duration::from_millis(50),
        ..fast_config()
    };
    let server = Server::start(config).await;
    let mut host = Client::create(server.addr, 3, "Alice").await;
    let guest = Client::join(server.addr, &host.room_id, "Bob", None).await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    drop(guest);

    let host = timeout(Duration::from_secs(60), play_to_end(host))
        .await
        .expect("game did not finish");
    assert!(host.game_over());
    // The guest's seat is still a human seat (it was only on autopilot).
    assert!(matches!(
        host.room.unwrap().seats[1],
        SeatView::Human {
            connected: false,
            ..
        }
    ));
    server.stop().await;
}

#[tokio::test]
async fn rooms_survive_a_restart() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("isle-server-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ServerConfig {
        data_dir: Some(dir.clone()),
        ..fast_config()
    };

    let server = Server::start(config.clone()).await;
    let mut host = Client::create(server.addr, 3, "Alice").await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    // Play into the main game so there is a real action log, and stop on
    // our own turn, when no bot is about to act.
    let mut acted = None;
    loop {
        let (view, legal) = (host.view.clone().unwrap(), host.legal.clone().unwrap());
        let my_turn = view.current == 0
            && matches!(view.phase, Phase::PreRoll | Phase::Main)
            && view.trade.is_none();
        if view.turn >= 6 && my_turn {
            break;
        }
        if acted != Some(legal.version)
            && let Some(a) = choose(&legal.actions)
        {
            acted = Some(legal.version);
            host.send(&ClientMsg::Act {
                version: legal.version,
                action: a,
            })
            .await;
        }
        host.recv().await;
    }
    let (room, token) = (host.room_id.clone(), host.token.clone());
    let (before_view, before_legal) = (host.view.clone().unwrap(), host.legal.clone().unwrap());
    drop(host);
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop().await;

    let server = Server::start(config).await;
    let mut back = Client::join(server.addr, &room, "x", token).await;
    back.until(|c| c.view.is_some()).await;
    assert_eq!(back.seat, Some(0));
    assert_eq!(back.view.clone().unwrap(), before_view);
    assert_eq!(back.legal.clone().unwrap(), before_legal);
    assert!(before_view.turn >= 6);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn permissions_and_errors() {
    let server = Server::start(fast_config()).await;
    let mut host = Client::create(server.addr, 3, "Alice").await;
    let mut guest = Client::join(server.addr, &host.room_id, "Bob", None).await;

    guest.send(&ClientMsg::Start).await;
    guest.until(|c| c.errors.contains(&"NotHost".into())).await;
    guest
        .send(&ClientMsg::SetSeat {
            seat: 2,
            bot: Some(BotLevel::Easy),
        })
        .await;
    guest.until(|c| c.errors.len() == 2).await;
    assert_eq!(guest.errors[1], "NotHost");
    host.send(&ClientMsg::SetSeat {
        seat: 1,
        bot: Some(BotLevel::Easy),
    })
    .await;
    host.until(|c| c.errors.contains(&"InvalidSeat".into()))
        .await;
    host.send(&ClientMsg::Act {
        version: 0,
        action: Action::RollDice,
    })
    .await;
    host.until(|c| c.errors.contains(&"NotStarted".into()))
        .await;

    let missing = Client::join(server.addr, "NOSUCHROOM", "Eve", None).await;
    assert_eq!(missing.errors, vec!["RoomNotFound".to_string()]);
    let forged = Client::join(server.addr, &host.room_id, "Mallory", Some("0".repeat(32))).await;
    assert_eq!(
        forged.seat,
        Some(2),
        "an unknown token just takes a free seat"
    );

    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.errors.contains(&"AlreadyStarted".into()))
        .await;

    let mut watcher = Client::join(server.addr, &host.room_id, "Carol", None).await;
    assert_eq!(watcher.seat, None, "late joiners watch");
    watcher.until(|c| c.view.is_some()).await;
    assert!(watcher.view.as_ref().unwrap().me.is_none());
    assert!(watcher.legal.as_ref().unwrap().actions.is_empty());
    watcher
        .send(&ClientMsg::Act {
            version: 0,
            action: Action::RollDice,
        })
        .await;
    watcher
        .until(|c| c.errors.contains(&"Spectator".into()))
        .await;

    // Out-of-turn actions are rejected by the rules.
    guest.until(|c| c.view.is_some()).await;
    guest
        .send(&ClientMsg::Act {
            version: 0,
            action: Action::BuildSettlement(0),
        })
        .await;
    guest
        .until(|c| c.errors.contains(&"NotYourTurn".into()))
        .await;

    guest
        .ws
        .send(Message::Text("{not json".into()))
        .await
        .unwrap();
    guest
        .until(|c| c.errors.contains(&"BadMessage".into()))
        .await;
    server.stop().await;
}

#[tokio::test]
async fn chat_is_broadcast_and_rate_limited() {
    let server = Server::start(fast_config()).await;
    let mut host = Client::create(server.addr, 2, "Alice").await;
    let mut guest = Client::join(server.addr, &host.room_id, "Bob", None).await;
    host.send(&ClientMsg::Chat {
        text: "  换木头吗？\n".into(),
    })
    .await;
    host.send(&ClientMsg::Chat {
        text: "again".into(),
    })
    .await;
    guest.until(|c| !c.chats.is_empty()).await;
    assert_eq!(
        guest.chats[0],
        ChatLine {
            seat: Some(0),
            name: "Alice".into(),
            text: "换木头吗？".into()
        }
    );
    host.until(|c| c.errors.contains(&"RateLimited".into()))
        .await;
    server.stop().await;
}

#[tokio::test]
async fn health_endpoint() {
    let server = Server::start(fast_config()).await;
    let _host = Client::create(server.addr, 2, "Alice").await;
    let mut tcp = TcpStream::connect(server.addr).await.unwrap();
    tcp.write_all(b"GET /api/health HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut body = String::new();
    tcp.read_to_string(&mut body).await.unwrap();
    assert!(body.starts_with("HTTP/1.1 200"), "{body}");
    assert!(body.contains("\"rooms\":1"), "{body}");
    server.stop().await;
}
