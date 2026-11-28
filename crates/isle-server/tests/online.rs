//! End-to-end tests over real WebSockets against a server on a random port.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::*;
use futures_util::SinkExt;
use isle_core::{Action, Event, Phase};
use isle_proto::net::{
    BotLevel, BotPersona, ChatLine, ClientMsg, HouseRules, MatchmakeRequest, MatchmakeResponse,
    QuickEmote, SeatView, ServerMsg,
};
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
        persona: BotPersona::Balanced,
    })
    .await;
    host.until(|c| {
        c.room.as_ref().unwrap().seats[3]
            == SeatView::Bot {
                level: BotLevel::Easy,
                persona: BotPersona::Balanced,
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
            level: BotLevel::Normal,
            persona: BotPersona::Balanced,
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
            persona: BotPersona::Balanced,
        })
        .await;
    guest.until(|c| c.errors.len() == 2).await;
    assert_eq!(guest.errors[1], "NotHost");
    host.send(&ClientMsg::SetSeat {
        seat: 1,
        bot: Some(BotLevel::Easy),
        persona: BotPersona::Balanced,
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
        is_public: false,
    })
    .await;
    host.until(|c| c.room.is_some()).await;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    assert!(host.room.as_ref().unwrap().seats[1..].iter().all(|s| *s
        == SeatView::Bot {
            level: BotLevel::Hard,
            persona: BotPersona::Balanced,
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
            is_public: false,
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
        is_public: false,
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
        is_public: false,
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
            event_deck: false,
            turn_timer_secs: 1,
            custom_map: None,
        },
        session: None,
        is_public: false,
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
    timeout(Duration::from_secs(20), async {
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
    timeout(Duration::from_secs(120), host.until(|c| c.game_over()))
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
        event_deck: true,
        turn_timer_secs: 60,
        custom_map: None,
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
                event_deck: false,
                turn_timer_secs: 0,
                custom_map: None,
            },
            session: None,
            is_public: false,
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
            is_public: false,
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

    // 4. Scenario::Fishermen + event_deck room playing to completion.
    let mut fish_host = Client::connect(server.addr).await;
    fish_host
        .send(&ClientMsg::Create {
            players: 4,
            bots: BotLevel::Normal,
            name: "Angler".into(),
            ranked: false,
            scenario: isle_core::Scenario::Fishermen,
            rules: HouseRules {
                victory_points: Some(8),
                friendly_robber: false,
                paired_turns: false,
                event_deck: true,
                turn_timer_secs: 0,
                custom_map: None,
            },
            session: None,
            is_public: false,
        })
        .await;
    fish_host.until(|c| c.room.is_some()).await;
    assert!(fish_host.room.as_ref().unwrap().rules.event_deck);
    fish_host.send(&ClientMsg::Start).await;
    fish_host.until(|c| c.view.is_some()).await;
    assert_eq!(
        fish_host.view.as_ref().unwrap().dice_deck_left,
        Some(isle_core::DICE_DECK_SIZE as u8)
    );
    let fish_host = timeout(Duration::from_secs(120), play_to_end(fish_host))
        .await
        .expect("fishermen game timed out");
    assert!(fish_host.game_over());
    assert!(
        fish_host
            .events
            .iter()
            .any(|e| matches!(e, Event::FishCaught { .. })),
        "expected FishCaught events in Fishermen room"
    );

    server.stop().await;
}

#[tokio::test]
async fn public_rooms_directory_kick_swap_rematch_and_custom_map() {
    use isle_core::{Board, BoardLayout, CustomMap, Rng, Scenario, Terrain};
    use isle_proto::net::PublicRoomSummary;

    fn assert_copy<T: Copy>() {}
    assert_copy::<HouseRules>();

    let server = Server::start(fast_config()).await;

    // Empty initially.
    let (status, body) = http_get(server.addr, "/api/rooms").await;
    assert!(status.contains("200"), "{status}");
    let list: Vec<PublicRoomSummary> = serde_json::from_str(&body).unwrap();
    assert!(list.is_empty());

    // Private room is hidden from `/api/rooms`.
    let mut priv_host = Client::create(server.addr, 3, "Hidden").await;
    assert!(!priv_host.room.as_ref().unwrap().is_public);
    let (_, body) = http_get(server.addr, "/api/rooms").await;
    assert!(serde_json::from_str::<Vec<PublicRoomSummary>>(&body)
        .unwrap()
        .is_empty());

    // Custom map room created with `is_public: true`.
    let mut cm = CustomMap::from_board(&Board::generate(&mut Rng::new(77), BoardLayout::Base, true));
    cm.terrain[0] = Terrain::Desert;
    cm.numbers[0] = 0;
    cm.terrain[1] = Terrain::Gold;
    cm.numbers[1] = 6;

    // Layout mismatch (5 players -> Extended, but `cm` is Base) is rejected on Create.
    let mut bad_create = Client::connect(server.addr).await;
    bad_create
        .send(&ClientMsg::Create {
            players: 5,
            bots: BotLevel::Normal,
            name: "Bad".into(),
            ranked: false,
            scenario: Scenario::Classic,
            rules: HouseRules {
                custom_map: Some(cm),
                ..Default::default()
            },
            session: None,
            is_public: true,
        })
        .await;
    bad_create.until(|c| !c.errors.is_empty()).await;
    assert_eq!(bad_create.errors, ["BadMessage"]);

    let mut host = Client::connect(server.addr).await;
    host.send(&ClientMsg::Create {
        players: 4,
        bots: BotLevel::Normal,
        name: "Captain".into(),
        ranked: false,
        scenario: Scenario::Classic,
        rules: HouseRules {
            victory_points: Some(6),
            friendly_robber: false,
            paired_turns: false,
            event_deck: false,
            turn_timer_secs: 0,
            custom_map: Some(cm),
        },
        session: None,
        is_public: true,
    })
    .await;
    host.until(|c| c.room.is_some()).await;
    assert!(host.room.as_ref().unwrap().is_public);
    assert_eq!(host.room.as_ref().unwrap().rules.custom_map, Some(cm));

    let mut guest = Client::join(server.addr, &host.room_id, "Mate", None).await;
    assert_eq!(guest.seat, Some(1));
    host.until(|c| matches!(c.room.as_ref().unwrap().seats[1], SeatView::Human { .. }))
        .await;

    // Directory shows 1 public room with humans=2, started=false, hasCustomMap=true.
    let (_, body) = http_get(server.addr, "/api/rooms").await;
    let list: Vec<PublicRoomSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, host.room_id);
    assert_eq!(list[0].host_name, "Captain");
    assert_eq!((list[0].humans, list[0].max_players), (2, 4));
    assert!(!list[0].started);
    assert!(list[0].has_custom_map);

    // Non-host cannot SetPublic, SwapSeats, or KickSeat.
    for msg in [
        ClientMsg::SetPublic { is_public: false },
        ClientMsg::SwapSeats { a: 0, b: 1 },
        ClientMsg::KickSeat { seat: 0 },
    ] {
        let prev = guest.errors.len();
        guest.send(&msg).await;
        guest.until(|c| c.errors.len() > prev).await;
        assert_eq!(guest.errors.last().unwrap(), "NotHost");
    }

    // SwapSeats: host swaps seat 0 (Captain) and seat 2 (Open) -> host moves to seat 2!
    host.send(&ClientMsg::SwapSeats { a: 0, b: 2 }).await;
    host.until(|c| c.seat == Some(2) && c.room.as_ref().unwrap().host == 2)
        .await;
    guest.until(|c| c.room.as_ref().unwrap().host == 2).await;
    assert_eq!(host.room.as_ref().unwrap().you, Some(2));
    assert_eq!(host.room.as_ref().unwrap().seats[0], SeatView::Open);

    // SwapSeats between two humans: seat 2 (Captain) and seat 1 (Mate).
    host.send(&ClientMsg::SwapSeats { a: 2, b: 1 }).await;
    host.until(|c| c.seat == Some(1) && c.room.as_ref().unwrap().host == 1)
        .await;
    guest
        .until(|c| c.seat == Some(2) && c.room.as_ref().unwrap().you == Some(2))
        .await;

    // KickSeat: host (at seat 1) kicks guest (at seat 2) -> guest becomes spectator with `Kicked`.
    host.send(&ClientMsg::KickSeat { seat: 2 }).await;
    guest
        .until(|c| c.seat.is_none() && c.errors.contains(&"Kicked".into()))
        .await;
    host.until(|c| {
        let r = c.room.as_ref().unwrap();
        r.seats[2] == SeatView::Open && r.spectators == 1
    })
    .await;
    assert!(guest.token.is_none());

    // Toggle private room to public so we can test sorting (`!started` before `started`).
    priv_host.send(&ClientMsg::SetPublic { is_public: true }).await;
    priv_host.until(|c| c.room.as_ref().unwrap().is_public).await;

    // Start the custom-map room -> now `priv_host` (!started) sorts before `host` (started).
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    let (_, body) = http_get(server.addr, "/api/rooms").await;
    let list: Vec<PublicRoomSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, priv_host.room_id);
    assert!(!list[0].started);
    assert_eq!(list[1].id, host.room_id);
    assert!(list[1].started);
    assert_eq!(list[1].spectators, 1);

    // SetPublic works in-game too.
    host.send(&ClientMsg::SetPublic { is_public: false }).await;
    host.until(|c| !c.room.as_ref().unwrap().is_public).await;
    let (_, body) = http_get(server.addr, "/api/rooms").await;
    let list: Vec<PublicRoomSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(list.len(), 1);
    host.send(&ClientMsg::SetPublic { is_public: true }).await;
    host.until(|c| c.room.as_ref().unwrap().is_public).await;

    // Rematch before game over is rejected with `NotFinished`.
    let prev = host.errors.len();
    host.send(&ClientMsg::Rematch { immediate: false }).await;
    host.until(|c| c.errors.len() > prev).await;
    assert_eq!(host.errors.last().unwrap(), "NotFinished");

    // Play game 1 to completion; finished room disappears from `/api/rooms`.
    let mut host = timeout(Duration::from_secs(60), play_to_end(host))
        .await
        .expect("custom map game 1 timed out");
    host.until(|c| c.room.as_ref().unwrap().replay.is_some())
        .await;
    let (_, body) = http_get(server.addr, "/api/rooms").await;
    let list: Vec<PublicRoomSummary> = serde_json::from_str(&body).unwrap();
    assert!(list.iter().all(|r| r.id != host.room_id));

    // Rematch (immediate: true) starts game 2 right away with fresh snapshot.
    host.view = None;
    host.send(&ClientMsg::Rematch { immediate: true }).await;
    host.until(|c| c.view.as_ref().is_some_and(|v| v.winner.is_none()))
        .await;
    assert!(host.room.as_ref().unwrap().started);
    assert!(host.room.as_ref().unwrap().replay.is_none());

    // Play game 2 to completion, then Rematch (immediate: false) returns to lobby!
    let mut host = timeout(Duration::from_secs(60), play_to_end(host))
        .await
        .expect("custom map game 2 timed out");
    host.until(|c| c.room.as_ref().unwrap().replay.is_some())
        .await;

    host.send(&ClientMsg::Rematch { immediate: false }).await;
    host.until(|c| !c.room.as_ref().unwrap().started).await;
    let r = host.room.as_ref().unwrap();
    assert!(r.replay.is_none());
    assert_eq!(r.seats[0], SeatView::Open);
    assert!(matches!(r.seats[1], SeatView::Human { .. }));

    // Back in `/api/rooms` as `!started`!
    let (_, body) = http_get(server.addr, "/api/rooms").await;
    let list: Vec<PublicRoomSummary> = serde_json::from_str(&body).unwrap();
    assert!(list.iter().any(|s| s.id == host.room_id && !s.started));

    server.stop().await;
}

fn record_bot_game(scenario: isle_core::Scenario, seed: u64) -> (isle_proto::ReplayRecord, isle_core::GameState) {
    use isle_ai::{Bot, GreedyBot};
    use isle_core::{GameConfig, GameState};
    use isle_proto::{ReplayPlayer, ReplayRecord};

    let config = GameConfig::scenario(scenario, 4);
    let mut g = GameState::new(config, seed).unwrap();
    let mut bots: Vec<GreedyBot> = (0..4).map(|k| GreedyBot::new(seed * 11 + k)).collect();
    let mut actions = Vec::new();
    let mut legal = Vec::new();
    while let Some(p) = g.to_move() {
        g.legal_actions(p, &mut legal);
        let a = bots[p as usize].choose(&g, p, &legal);
        g.apply(p, a).unwrap();
        actions.push((p, a));
    }
    let players = (0..4)
        .map(|i| ReplayPlayer {
            name: if i == 0 { "Player".into() } else { String::new() },
            bot: (i != 0).then_some(BotLevel::Normal),
        })
        .collect();
    (
        ReplayRecord::new(config, seed, players, &actions, g.winner),
        g,
    )
}

#[tokio::test]
async fn daily_challenge_anti_cheat_leaderboard_and_caravans_room() {
    use isle_proto::account::{Credentials, SessionGrant};
    use isle_proto::daily::{DailyLeaderboard, DailySubmitRequest};

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("daily-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ServerConfig {
        data_dir: Some(dir.clone()),
        auth_burst: 0,
        ..fast_config()
    };
    let server = Server::start(config.clone()).await;

    // 1. Date validation on GET /api/daily.
    for bad_path in ["/api/daily", "/api/daily?date=bad", "/api/daily?date=2026-13-01"] {
        let (status, _, _) = http(server.addr, "GET", bad_path, &[], None).await;
        assert_eq!(status, 400, "{bad_path}");
    }
    let (status, _, body) = http(server.addr, "GET", "/api/daily?date=2026-11-28", &[], None).await;
    assert_eq!(status, 200);
    let empty: DailyLeaderboard = serde_json::from_str(&body).unwrap();
    assert_eq!(empty.date_key, "2026-11-28");
    assert!(empty.entries.is_empty());

    // Build two distinct finished replays (pick winning seat for fast & slow wins, plus a losing seat).
    let (rep_a, end_a) = record_bot_game(isle_core::Scenario::Classic, 10);
    let (rep_b, end_b) = record_bot_game(isle_core::Scenario::Caravans, 25);
    let win_seat_a = end_a.winner.unwrap();
    let lose_seat_a = (win_seat_a + 1) % 4;
    let win_seat_b = end_b.winner.unwrap();

    // 2. Tampered replay (corrupt action byte) and unfinished replay are rejected with 400!
    let mut tampered = rep_a.clone();
    tampered.actions[1] ^= 0x7F;
    let bad_req = DailySubmitRequest {
        date_key: "2026-11-28".into(),
        seat: win_seat_a,
        player_name: "Cheater".into(),
        replay: tampered,
    };
    let (status, _, _) = http(
        server.addr,
        "POST",
        "/api/daily",
        &[],
        Some(&serde_json::to_string(&bad_req).unwrap()),
    )
    .await;
    assert_eq!(status, 400, "tampered replay must be rejected");

    let mut unfinished = rep_a.clone();
    let half_actions = &rep_a.actions().unwrap()[..12];
    unfinished.actions = isle_proto::replay::encode_actions(half_actions);
    unfinished.winner = None;
    let unfin_req = DailySubmitRequest {
        date_key: "2026-11-28".into(),
        seat: win_seat_a,
        player_name: "Quitter".into(),
        replay: unfinished,
    };
    let (status, _, _) = http(
        server.addr,
        "POST",
        "/api/daily",
        &[],
        Some(&serde_json::to_string(&unfin_req).unwrap()),
    )
    .await;
    assert_eq!(status, 400, "unfinished replay must be rejected");

    // 3. Guest loss submitted first, then upgraded by a guest win under same canonical name ("alice").
    let loss_req = DailySubmitRequest {
        date_key: "2026-11-28".into(),
        seat: lose_seat_a,
        player_name: "  Alice \n".into(),
        replay: rep_a.clone(),
    };
    let (status, _, body) = http(
        server.addr,
        "POST",
        "/api/daily",
        &[],
        Some(&serde_json::to_string(&loss_req).unwrap()),
    )
    .await;
    assert_eq!(status, 200);
    let lb1: DailyLeaderboard = serde_json::from_str(&body).unwrap();
    assert_eq!(lb1.entries.len(), 1);
    assert_eq!(lb1.entries[0].player_name, "Alice");
    assert!(!lb1.entries[0].registered);
    assert!(!lb1.entries[0].won);
    assert_eq!(lb1.entries[0].rank, 1);

    // Upgrade "alice" with a winning run -> deduplicates and replaces the loss!
    let win_req = DailySubmitRequest {
        date_key: "2026-11-28".into(),
        seat: win_seat_a,
        player_name: "alice".into(),
        replay: rep_a.clone(),
    };
    let (status, _, body) = http(
        server.addr,
        "POST",
        "/api/daily",
        &[],
        Some(&serde_json::to_string(&win_req).unwrap()),
    )
    .await;
    assert_eq!(status, 200);
    let lb2: DailyLeaderboard = serde_json::from_str(&body).unwrap();
    assert_eq!(lb2.entries.len(), 1);
    assert!(lb2.entries[0].won);
    assert_eq!(lb2.entries[0].turns, end_a.turn);
    assert_eq!(lb2.entries[0].points, end_a.victory_points(win_seat_a));

    // Re-submitting the loss does NOT overwrite the better winning run!
    let (status, _, body) = http(
        server.addr,
        "POST",
        "/api/daily",
        &[],
        Some(&serde_json::to_string(&loss_req).unwrap()),
    )
    .await;
    assert_eq!(status, 200);
    let lb_keep: DailyLeaderboard = serde_json::from_str(&body).unwrap();
    assert_eq!(lb_keep.entries.len(), 1);
    assert!(lb_keep.entries[0].won);

    // 4. Signed-in account submission uses verified `session.account.username` and `registered = true`.
    let creds = Credentials {
        username: "SpeedRunner".into(),
        password: "correct-horse-battery".into(),
    };
    let (status, _, grant_body) = http(
        server.addr,
        "POST",
        "/api/accounts",
        &[],
        Some(&serde_json::to_string(&creds).unwrap()),
    )
    .await;
    assert_eq!(status, 201);
    let grant: SessionGrant = serde_json::from_str(&grant_body).unwrap();
    let auth_header = format!("Bearer {}", grant.token);
    let auth_req = DailySubmitRequest {
        date_key: "2026-11-28".into(),
        seat: win_seat_b,
        player_name: "SpoofedName".into(),
        replay: rep_b,
    };
    let (status, _, body) = http(
        server.addr,
        "POST",
        "/api/daily",
        &[("Authorization", &auth_header)],
        Some(&serde_json::to_string(&auth_req).unwrap()),
    )
    .await;
    assert_eq!(status, 200);
    let lb3: DailyLeaderboard = serde_json::from_str(&body).unwrap();
    assert_eq!(lb3.entries.len(), 2);
    assert_eq!(lb3.entries[0].rank, 1);
    assert_eq!(lb3.entries[1].rank, 2);
    assert!(lb3.entries[0].turns <= lb3.entries[1].turns);
    assert!(
        lb3.entries
            .iter()
            .any(|e| e.player_name == "speedrunner" && e.registered)
    );

    // 5. Scenario::Caravans online room plays cleanly to completion.
    let mut caravan_host = Client::connect(server.addr).await;
    caravan_host
        .send(&ClientMsg::Create {
            players: 4,
            bots: BotLevel::Normal,
            name: "Nomad".into(),
            ranked: false,
            scenario: isle_core::Scenario::Caravans,
            rules: HouseRules {
                victory_points: Some(8),
                friendly_robber: false,
                paired_turns: false,
                event_deck: false,
                turn_timer_secs: 0,
                custom_map: None,
            },
            session: None,
            is_public: false,
        })
        .await;
    caravan_host.until(|c| c.room.is_some()).await;
    caravan_host.send(&ClientMsg::Start).await;
    caravan_host.until(|c| c.view.is_some()).await;
    let caravan_host = timeout(Duration::from_secs(90), play_to_end(caravan_host))
        .await
        .expect("caravans online game timed out");
    assert!(caravan_host.game_over());
    assert!(
        caravan_host
            .events
            .iter()
            .any(|e| matches!(e, Event::CamelPlaced { .. })),
        "expected CamelPlaced events in Caravans room"
    );

    // 6. Restart server on the same data_dir and verify `daily.json` persisted!
    server.stop().await;
    assert!(dir.join("daily.json").exists());
    let server2 = Server::start(config).await;
    let (status, _, body2) =
        http(server2.addr, "GET", "/api/daily?date=2026-11-28", &[], None).await;
    assert_eq!(status, 200);
    let lb_reloaded: DailyLeaderboard = serde_json::from_str(&body2).unwrap();
    assert_eq!(lb_reloaded, lb3);
    server2.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn matchmaking_bot_personas_quick_emotes_and_explorers_room() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("matchmake-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let config = ServerConfig {
        data_dir: Some(dir.clone()),
        hard_bot: "mcts:32".into(),
        ..fast_config()
    };
    let server = Server::start(config.clone()).await;

    // 1. Player count validation on POST /api/matchmake.
    for bad_players in [0u8, 1, 7] {
        let req = MatchmakeRequest {
            scenario: isle_core::Scenario::Explorers,
            players: bad_players,
            ranked: false,
        };
        let (status, _, _) = http(
            server.addr,
            "POST",
            "/api/matchmake",
            &[],
            Some(&serde_json::to_string(&req).unwrap()),
        )
        .await;
        assert_eq!(status, 400, "players={bad_players}");
    }

    // 2. Two clients call POST /api/matchmake with identical (scenario, players, ranked):
    // Client 1 creates the waiting room; Client 2 gets the SAME roomId with `created == false`.
    let req = MatchmakeRequest {
        scenario: isle_core::Scenario::Explorers,
        players: 4,
        ranked: false,
    };
    let req_json = serde_json::to_string(&req).unwrap();
    let (s1, _, b1) = http(server.addr, "POST", "/api/matchmake", &[], Some(&req_json)).await;
    assert_eq!(s1, 200);
    let m1: MatchmakeResponse = serde_json::from_str(&b1).unwrap();
    assert!(m1.created);

    let (s2, _, b2) = http(server.addr, "POST", "/api/matchmake", &[], Some(&req_json)).await;
    assert_eq!(s2, 200);
    let m2: MatchmakeResponse = serde_json::from_str(&b2).unwrap();
    assert!(!m2.created);
    assert_eq!(m1.room_id, m2.room_id);

    // Different queue key (ranked: true) gets a distinct waiting room.
    let ranked_req = MatchmakeRequest {
        scenario: isle_core::Scenario::Explorers,
        players: 4,
        ranked: true,
    };
    let (sr, _, br) = http(
        server.addr,
        "POST",
        "/api/matchmake",
        &[],
        Some(&serde_json::to_string(&ranked_req).unwrap()),
    )
    .await;
    assert_eq!(sr, 200);
    let mr: MatchmakeResponse = serde_json::from_str(&br).unwrap();
    assert!(mr.created);
    assert_ne!(mr.room_id, m1.room_id);

    // Client 1 joins first -> becomes host on seat 0! Client 2 joins -> takes seat 1!
    let mut c1 = Client::join(server.addr, &m1.room_id, "Drake", None).await;
    assert_eq!(c1.seat, Some(0));
    assert_eq!(c1.room.as_ref().unwrap().host, 0);

    let mut c2 = Client::join(server.addr, &m2.room_id, "Magellan", None).await;
    assert_eq!(c2.seat, Some(1));
    assert_eq!(c2.room.as_ref().unwrap().host, 0);
    c1.until(|c| matches!(c.room.as_ref().unwrap().seats[1], SeatView::Human { .. }))
        .await;

    // 3. Per-seat BotPersona via SetSeat (and filling all remaining Open seats so matchmaking opens a new room).
    c1.send(&ClientMsg::SetSeat {
        seat: 2,
        bot: Some(BotLevel::Normal),
        persona: BotPersona::Merchant,
    })
    .await;
    c1.send(&ClientMsg::SetSeat {
        seat: 3,
        bot: Some(BotLevel::Hard),
        persona: BotPersona::Warlord,
    })
    .await;
    c1.until(|c| {
        let r = c.room.as_ref().unwrap();
        r.seats[2]
            == SeatView::Bot {
                level: BotLevel::Normal,
                persona: BotPersona::Merchant,
            }
            && r.seats[3]
                == SeatView::Bot {
                    level: BotLevel::Hard,
                    persona: BotPersona::Warlord,
                }
    })
    .await;

    // Now room `m1.room_id` has 0 Open seats -> next matchmake creates a new room!
    let (s3, _, b3) = http(server.addr, "POST", "/api/matchmake", &[], Some(&req_json)).await;
    assert_eq!(s3, 200);
    let m3: MatchmakeResponse = serde_json::from_str(&b3).unwrap();
    assert!(m3.created);
    assert_ne!(m3.room_id, m1.room_id);

    // 4. QuickChat broadcast and rate-limit guard.
    c1.send(&ClientMsg::QuickChat {
        emote: QuickEmote::OpenToTrade,
    })
    .await;
    c1.send(&ClientMsg::QuickChat {
        emote: QuickEmote::WellPlayed,
    })
    .await;
    c2.until(|c| !c.quick_chats.is_empty()).await;
    assert_eq!(
        c2.quick_chats[0],
        (Some(0), "Drake".into(), QuickEmote::OpenToTrade)
    );
    c1.until(|c| c.errors.contains(&"RateLimited".into())).await;

    // Lower VP to 10 for fast online completion, start Explorers game, verify BoardInfo sites!
    c1.send(&ClientMsg::SetRules {
        rules: HouseRules {
            victory_points: Some(10),
            friendly_robber: false,
            paired_turns: false,
            event_deck: true,
            turn_timer_secs: 0,
            custom_map: None,
        },
    })
    .await;
    c1.until(|c| c.room.as_ref().unwrap().rules.victory_points == Some(10))
        .await;

    c1.send(&ClientMsg::Start).await;
    let mut c1_board = None;
    while c1.view.is_none() {
        if let ServerMsg::Snapshot(snap) = c1.recv().await {
            c1_board = Some(snap.board);
        }
    }
    c2.until(|c| c.view.is_some()).await;
    let bi = c1_board.expect("snapshot board");
    assert_eq!(bi.pirate_lairs.len(), 3);
    assert_eq!(bi.deep_shoals.len(), 3);
    assert!(bi.spice_islands.len() >= 3);

    // Verify BotPersona survives server restart persistence!
    let (saved_room, t1, t2) = (c1.room_id.clone(), c1.token.clone(), c2.token.clone());
    drop(c1);
    drop(c2);
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop().await;

    let server = Server::start(config).await;
    let mut c1 = Client::join(server.addr, &saved_room, "x", t1).await;
    let mut c2 = Client::join(server.addr, &saved_room, "y", t2).await;
    c1.until(|c| c.view.is_some()).await;
    c2.until(|c| c.view.is_some()).await;
    assert_eq!(
        c1.room.as_ref().unwrap().seats[2],
        SeatView::Bot {
            level: BotLevel::Normal,
            persona: BotPersona::Merchant,
        }
    );
    assert_eq!(
        c1.room.as_ref().unwrap().seats[3],
        SeatView::Bot {
            level: BotLevel::Hard,
            persona: BotPersona::Warlord,
        }
    );

    let (c1, c2) = tokio::join!(
        timeout(Duration::from_secs(120), play_to_end(c1)),
        timeout(Duration::from_secs(120), play_to_end(c2))
    );
    let (c1, c2) = (c1.expect("c1 timed out"), c2.expect("c2 timed out"));
    assert!(c1.game_over() && c2.game_over());
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

