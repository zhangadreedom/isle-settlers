//! End-to-end tests over real WebSockets against a server on a random port.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use futures_util::SinkExt;
use isle_core::{Action, Event, Phase};
use isle_proto::net::{BotLevel, ChatLine, ClientMsg, HouseRules, SeatView, ServerMsg};
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
        ranked: false,
        scenario: Default::default(),
        rules: Default::default(),
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

#[tokio::test]
async fn archipelago_rooms_play_out_and_restore() {
    let dir = temp_dir("archipelago");
    let config = ServerConfig {
        data_dir: Some(dir.clone()),
        ..fast_config()
    };
    let server = Server::start(config.clone()).await;
    // Every scenario seats at most six.
    let mut too_big = Client::connect(server.addr).await;
    too_big
        .send(&ClientMsg::Create {
            players: 7,
            bots: BotLevel::Normal,
            name: "A".into(),
            ranked: false,
            scenario: isle_core::Scenario::Archipelago,
            rules: Default::default(),
            session: None,
        })
        .await;
    too_big.until(|c| !c.errors.is_empty()).await;
    assert_eq!(too_big.errors, ["BadMessage"]);

    let mut host = Client::connect(server.addr).await;
    host.send(&ClientMsg::Create {
        players: 3,
        bots: BotLevel::Normal,
        name: "Ann".into(),
        ranked: false,
        scenario: isle_core::Scenario::Archipelago,
        rules: Default::default(),
        session: None,
    })
    .await;
    host.until(|c| c.room.is_some()).await;
    assert_eq!(
        host.room.as_ref().unwrap().scenario,
        isle_core::Scenario::Archipelago
    );
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    let view = host.view.clone().unwrap();
    assert!(view.pirate.is_some());
    assert_eq!(view.ships.len(), view.roads.len());
    // A restart rebuilds the seafaring game.
    let (room, token) = (host.room_id.clone(), host.token.clone());
    drop(host);
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop().await;
    let server = Server::start(config).await;
    let back = Client::join(server.addr, &room, "x", token).await;
    let back = timeout(Duration::from_secs(60), async {
        let mut c = back;
        c.until(|c| c.view.is_some()).await;
        c
    })
    .await
    .unwrap();
    assert_eq!(back.view.as_ref().unwrap(), &view);
    let end = timeout(Duration::from_secs(120), play_to_end(back))
        .await
        .expect("finished");
    let view = end.view.as_ref().unwrap();
    assert!(view.winner.is_some());
    assert_eq!(
        end.room.as_ref().unwrap().scenario,
        isle_core::Scenario::Archipelago
    );
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn fog_islands_rooms_play_out_and_reveal_tiles() {
    let dir = temp_dir("fog-islands");
    let config = ServerConfig {
        data_dir: Some(dir.clone()),
        ..fast_config()
    };
    let server = Server::start(config.clone()).await;
    let mut host = Client::connect(server.addr).await;
    host.send(&ClientMsg::Create {
        players: 4,
        bots: BotLevel::Normal,
        name: "Captain".into(),
        ranked: false,
        scenario: isle_core::Scenario::FogIslands,
        rules: Default::default(),
        session: None,
    })
    .await;
    host.until(|c| c.room.is_some()).await;
    assert_eq!(
        host.room.as_ref().unwrap().scenario,
        isle_core::Scenario::FogIslands
    );
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    let initial_fog = host
        .view
        .as_ref()
        .unwrap()
        .fog
        .as_ref()
        .expect("fog view present")
        .terrain
        .iter()
        .filter(|&&t| t == isle_core::Terrain::Fog)
        .count();
    assert_eq!(initial_fog, 13);

    let end = timeout(Duration::from_secs(120), async move {
        let mut c = host;
        let mut acted_at = None;
        while !c.game_over() {
            if let Some(legal) = c.legal.clone()
                && acted_at != Some(legal.version)
            {
                let rank = |a: &Action| match a {
                    Action::BuildCity(_) => 0,
                    Action::BuildSettlement(_) => 1,
                    Action::BuildShip(_) => 2,
                    Action::BuildRoad(_) => 3,
                    Action::RollDice => 4,
                    Action::Discard(_) | Action::ChooseGold(_) | Action::MoveRobber { .. } => 5,
                    Action::RespondTrade { accept: false } => 6,
                    Action::EndTurn => 7,
                    _ => 9,
                };
                if let Some(action) = legal.actions.iter().copied().min_by_key(rank) {
                    acted_at = Some(legal.version);
                    c.send(&ClientMsg::Act {
                        version: legal.version,
                        action,
                    })
                    .await;
                }
            }
            c.recv().await;
        }
        c
    })
    .await
    .expect("finished");
    let view = end.view.as_ref().unwrap();
    assert!(view.winner.is_some());
    let final_fog = view
        .fog
        .as_ref()
        .unwrap()
        .terrain
        .iter()
        .filter(|&&t| t == isle_core::Terrain::Fog)
        .count();
    assert!(final_fog < 13, "expected fog tiles to be uncovered, still {final_fog}");
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn turn_timer_auto_acts_for_idle_connected_player() {
    let server = Server::start(fast_config()).await;
    let mut host = Client::connect(server.addr).await;
    host.send(&ClientMsg::Create {
        players: 4,
        bots: BotLevel::Normal,
        name: "Idle".into(),
        ranked: false,
        scenario: isle_core::Scenario::Classic,
        rules: HouseRules {
            victory_points: Some(6),
            friendly_robber: false,
            paired_turns: false,
            turn_timer_secs: 1,
        },
        session: None,
    })
    .await;
    host.until(|c| c.room.is_some()).await;
    assert_eq!(host.room.as_ref().unwrap().rules.turn_timer_secs, 1);

    host.send(&ClientMsg::Start).await;
    let mut snapshot_timer = None;
    while host.view.is_none() {
        if let ServerMsg::Snapshot(s) = host.recv().await {
            snapshot_timer = s.turn_remaining_ms;
        }
    }
    assert!(
        snapshot_timer.is_some_and(|ms| ms > 0 && ms <= 1000),
        "expected snapshot timer, got {snapshot_timer:?}"
    );

    // Connected human sits completely idle: within a few seconds the 1s turn
    // timer should automatically execute their setup actions and advance play.
    let mut saw_update_timer = false;
    timeout(Duration::from_secs(6), async {
        while host.legal.as_ref().unwrap().version < 4 {
            if let ServerMsg::Update(u) = host.recv().await
                && u.turn_remaining_ms.is_some()
            {
                saw_update_timer = true;
            }
        }
    })
    .await
    .expect("idle human turn did not advance automatically");
    assert!(saw_update_timer);

    // Keep sitting idle until the game finishes on its own.
    timeout(Duration::from_secs(60), host.until(|c| c.game_over()))
        .await
        .expect("timed game did not finish");
    assert!(host.view.as_ref().unwrap().winner.is_some());
    server.stop().await;
}

#[tokio::test]
async fn house_rules_and_citadels_and_six_player_seafaring_rooms() {
    let server = Server::start(fast_config()).await;

    // 1. SetRules validation, permissions, and Paired Turns + Friendly Robber.
    let mut host = Client::create(server.addr, 5, "Host").await;
    let mut guest = Client::join(server.addr, &host.room_id, "Guest", None).await;
    guest
        .send(&ClientMsg::SetRules {
            rules: HouseRules::default(),
        })
        .await;
    guest.until(|c| c.errors.contains(&"NotHost".into())).await;

    for bad in [
        HouseRules {
            victory_points: Some(5),
            ..Default::default()
        },
        HouseRules {
            victory_points: Some(17),
            ..Default::default()
        },
        HouseRules {
            turn_timer_secs: 301,
            ..Default::default()
        },
    ] {
        let prev = host.errors.len();
        host.send(&ClientMsg::SetRules { rules: bad }).await;
        host.until(|c| c.errors.len() > prev).await;
        assert_eq!(host.errors.last().unwrap(), "BadMessage");
    }

    let custom = HouseRules {
        victory_points: Some(7),
        friendly_robber: true,
        paired_turns: true,
        turn_timer_secs: 60,
    };
    host.send(&ClientMsg::SetRules { rules: custom }).await;
    host.until(|c| c.room.as_ref().unwrap().rules == custom)
        .await;
    guest
        .until(|c| c.room.as_ref().unwrap().rules == custom)
        .await;

    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    guest.until(|c| c.view.is_some()).await;
    let prev = host.errors.len();
    host.send(&ClientMsg::SetRules {
        rules: HouseRules::default(),
    })
    .await;
    host.until(|c| c.errors.len() > prev).await;
    assert_eq!(host.errors.last().unwrap(), "AlreadyStarted");

    let (host, _guest) = tokio::join!(
        timeout(Duration::from_secs(90), play_to_end(host)),
        timeout(Duration::from_secs(90), play_to_end(guest))
    );
    let host = host.expect("paired-turns game timed out");
    assert!(host.game_over());
    assert!(
        host.events
            .iter()
            .any(|e| matches!(e, Event::PairedTurnEnded { .. })),
        "expected PairedTurnEnded events under paired_turns"
    );

    // 2. 6-player Scenario::Archipelago room playing to completion.
    let mut sea_host = Client::connect(server.addr).await;
    sea_host
        .send(&ClientMsg::Create {
            players: 6,
            bots: BotLevel::Normal,
            name: "Admiral".into(),
            ranked: false,
            scenario: isle_core::Scenario::Archipelago,
            rules: HouseRules {
                victory_points: Some(8),
                friendly_robber: true,
                paired_turns: false,
                turn_timer_secs: 0,
            },
            session: None,
        })
        .await;
    sea_host.until(|c| c.room.is_some()).await;
    sea_host.send(&ClientMsg::Start).await;
    sea_host.until(|c| c.view.is_some()).await;
    let sv = sea_host.view.as_ref().unwrap();
    assert_eq!(
        (
            sv.players.len(),
            sv.buildings.len(),
            sv.roads.len(),
            sv.ships.len(),
        ),
        (6, 120, 165, 165)
    );
    assert!(sv.pirate.is_some());
    let sea_host = timeout(Duration::from_secs(120), play_to_end(sea_host))
        .await
        .expect("6p archipelago timed out");
    assert!(sea_host.game_over());

    // 3. Scenario::Citadels room playing to completion.
    let mut cit_host = Client::connect(server.addr).await;
    cit_host
        .send(&ClientMsg::Create {
            players: 4,
            bots: BotLevel::Normal,
            name: "Lord".into(),
            ranked: false,
            scenario: isle_core::Scenario::Citadels,
            rules: HouseRules::default(),
            session: None,
        })
        .await;
    cit_host.until(|c| c.room.is_some()).await;
    cit_host.send(&ClientMsg::Start).await;
    cit_host.until(|c| c.view.is_some()).await;
    {
        let cv = cit_host.view.as_ref().unwrap();
        let cit = cv.citadels.as_ref().expect("citadel view present");
        assert_eq!(cit.barbarian_steps, isle_core::BARBARIAN_STEPS);
        assert_eq!(cit.commodity_bank.0, [12, 12, 12]);
        assert_eq!(cv.me.as_ref().unwrap().commodity_trade_ratio, 4);
    }
    let cit_host = timeout(Duration::from_secs(120), play_to_end(cit_host))
        .await
        .expect("citadels game timed out");
    assert!(cit_host.game_over());
    for e in &cit_host.events {
        if let Event::CommodityStolen {
            thief,
            victim,
            commodity,
        } = e
            && *thief != 0
            && *victim != 0
        {
            assert!(commodity.is_none());
        }
    }

    server.stop().await;
}
