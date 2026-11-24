//! End-to-end tests over real WebSockets against a server on a random port.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use futures_util::SinkExt;
use isle_core::{Action, Event, Phase};
use isle_proto::net::{BotLevel, ChatLine, ClientMsg, SeatView};
use isle_server::ServerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

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

#[tokio::test]
async fn host_role_moves_on_if_the_host_leaves_the_lobby() {
    let server = Server::start(fast_config()).await;
    let host = Client::create(server.addr, 3, "Alice").await;
    let mut guest = Client::join(server.addr, &host.room_id, "Bob", None).await;
    drop(host);
    guest.until(|c| c.room.as_ref().unwrap().host == 1).await;
    guest.send(&ClientMsg::Start).await;
    guest.until(|c| c.view.is_some()).await;
    assert!(guest.room.as_ref().unwrap().started);
    server.stop().await;
}

#[tokio::test]
async fn hard_bots_play_on_the_blocking_pool() {
    let config = ServerConfig {
        hard_bot: "mcts:32".into(),
        ..fast_config()
    };
    let server = Server::start(config).await;
    let mut host = Client::connect(server.addr).await;
    host.send(&ClientMsg::Create {
        players: 4,
        bots: BotLevel::Hard,
        name: "Alice".into(),
        session: None,
    })
    .await;
    host.until(|c| c.room.is_some()).await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    assert!(host.room.as_ref().unwrap().seats[1..].iter().all(|s| *s
        == SeatView::Bot {
            level: BotLevel::Hard
        }));
    let host = timeout(Duration::from_secs(120), play_to_end(host))
        .await
        .expect("game did not finish");
    assert!(host.game_over());
    server.stop().await;
}

#[tokio::test]
async fn six_seat_rooms_play_on_the_extended_island() {
    let server = Server::start(fast_config()).await;
    let host = Client::create(server.addr, 6, "Alice").await;
    assert_eq!(host.room.as_ref().unwrap().seats.len(), 6);
    let mut guest = Client::join(server.addr, &host.room_id, "Bob", None).await;
    assert_eq!(guest.seat, Some(1));
    let mut host = host;
    host.until(|c| matches!(c.room.as_ref().unwrap().seats[1], SeatView::Human { .. }))
        .await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    guest.until(|c| c.view.is_some()).await;
    let view = host.view.as_ref().unwrap();
    assert_eq!(
        (view.players.len(), view.buildings.len(), view.roads.len()),
        (6, 80, 109)
    );

    let (host, guest) = tokio::join!(
        timeout(Duration::from_secs(90), play_to_end(host)),
        timeout(Duration::from_secs(90), play_to_end(guest))
    );
    let (host, guest) = (
        host.expect("host timed out"),
        guest.expect("guest timed out"),
    );
    assert_eq!(
        host.view.as_ref().unwrap().winner,
        guest.view.as_ref().unwrap().winner
    );
    assert_eq!(
        host.view
            .as_ref()
            .unwrap()
            .final_points
            .as_ref()
            .unwrap()
            .len(),
        6
    );
    // Every turn ends with the other five seats passing or building.
    let passes = host
        .events
        .iter()
        .filter(|e| matches!(e, Event::SpecialBuildPassed { .. }))
        .count();
    let turns = host
        .events
        .iter()
        .filter(|e| matches!(e, Event::TurnEnded { .. }))
        .count();
    assert!(
        turns > 0 && passes >= 5 * (turns - 1),
        "{passes} passes over {turns} turns"
    );
    server.stop().await;
}

#[tokio::test]
async fn finished_games_are_archived_as_replays() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("isle-server-replays-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ServerConfig {
        data_dir: Some(dir.clone()),
        ..fast_config()
    };
    let server = Server::start(config.clone()).await;
    let mut host = Client::create(server.addr, 3, "Alice").await;
    assert_eq!(host.room.as_ref().unwrap().replay, None);
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    let mut host = timeout(Duration::from_secs(60), play_to_end(host))
        .await
        .expect("game did not finish");
    host.until(|c| c.room.as_ref().unwrap().replay.is_some())
        .await;
    let id = host.room.as_ref().unwrap().replay.clone().unwrap();
    let winner = host.view.as_ref().unwrap().winner;

    let (status, body) =
        http_get(server.addr, &format!("/api/replays/{}", id.to_lowercase())).await;
    assert!(status.contains("200"), "{status}");
    assert!(!body.contains("token"), "seat tokens must not leak");
    let record: isle_proto::ReplayRecord = serde_json::from_str(&body).unwrap();
    assert_eq!(record.players[0].name, "Alice");
    assert_eq!(record.players[1].bot, Some(BotLevel::Normal));
    assert_eq!(record.winner, winner);
    let end = record.play(|_, _, _, _| {}).unwrap();
    assert_eq!(end.winner, winner);
    let (status, _) = http_get(server.addr, "/api/replays/NOPE2345").await;
    assert!(status.contains("404"), "{status}");
    drop(host);
    server.stop().await;

    // Replays outlive the server (read back from disk).
    let server = Server::start(config).await;
    let (status, again) = http_get(server.addr, &format!("/api/replays/{id}")).await;
    assert!(status.contains("200"), "{status}");
    assert_eq!(again, body);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}
