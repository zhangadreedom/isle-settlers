//! Account API and account-bound seats, against a server on a random port.

mod common;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use common::*;
use isle_proto::net::{ClientMsg, SeatView};
use isle_server::ServerConfig;
use serde_json::{Value, json};

const PW: &str = "correct horse battery";

fn accounts_config(dir: PathBuf) -> ServerConfig {
    ServerConfig {
        data_dir: Some(dir),
        auth_burst: 0,
        ..fast_config()
    }
}

async fn call(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, String, Value) {
    let auth = token.map(|t| format!("Bearer {t}"));
    let headers: Vec<(&str, &str)> = auth
        .as_deref()
        .map(|a| ("Authorization", a))
        .into_iter()
        .collect();
    let body = body.map(|b| b.to_string());
    let (status, head, body) = http(addr, method, path, &headers, body.as_deref()).await;
    let json = serde_json::from_str(&body).unwrap_or(Value::Null);
    (status, head, json)
}

fn creds(username: &str, password: &str) -> Option<Value> {
    Some(json!({ "username": username, "password": password }))
}

async fn register(addr: SocketAddr, username: &str) -> String {
    let (status, _, body) = call(addr, "POST", "/api/accounts", None, creds(username, PW)).await;
    assert_eq!(status, 201, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

async fn login(addr: SocketAddr, username: &str, password: &str) -> (u16, Value) {
    let (status, _, body) = call(
        addr,
        "POST",
        "/api/sessions",
        None,
        creds(username, password),
    )
    .await;
    (status, body)
}

fn seat(c: &Client, i: usize) -> (String, bool) {
    match &c.room.as_ref().unwrap().seats[i] {
        SeatView::Human {
            name, registered, ..
        } => (name.clone(), *registered),
        other => panic!("seat {i} is {other:?}"),
    }
}

#[tokio::test]
async fn accounts_need_a_data_directory() {
    let dir = temp_dir("no-accounts");
    for config in [
        fast_config(),
        ServerConfig {
            data_dir: Some(dir.clone()),
            accounts: false,
            ..fast_config()
        },
    ] {
        let server = Server::start(config).await;
        let (_, health) = http_get(server.addr, "/api/health").await;
        assert!(health.contains(r#""accounts":false"#), "{health}");
        let (status, _, body) =
            call(server.addr, "POST", "/api/accounts", None, creds("ann", PW)).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (503, Some("AccountsDisabled"))
        );
        let (status, _, _) = call(server.addr, "GET", "/api/session", Some("x"), None).await;
        assert_eq!(status, 503);
        // A session sent over the WebSocket is refused, not ignored.
        let c = Client::create_as(server.addr, 3, "Ann", Some("ab")).await;
        assert_eq!(c.errors, ["AccountsDisabled"]);
        assert!(c.room.is_none());
        server.stop().await;
    }
    assert!(!dir.join("accounts.sqlite3").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn register_log_in_and_log_out() {
    let dir = temp_dir("accounts-api");
    let server = Server::start(accounts_config(dir.clone())).await;
    let addr = server.addr;
    let (_, health) = http_get(addr, "/api/health").await;
    assert!(health.contains(r#""accounts":true"#), "{health}");

    // Registering logs in; names are canonicalized.
    let (status, head, body) =
        call(addr, "POST", "/api/accounts", None, creds(" Ann_01 ", PW)).await;
    assert_eq!(status, 201, "{body}");
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store"),
        "{head}"
    );
    assert_eq!(body["account"]["username"], "ann_01");
    let first = body["token"].as_str().unwrap().to_owned();
    assert_eq!(first.len(), 64);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert!(body["expiresAt"].as_i64().unwrap() > now + 29 * 24 * 3600);

    let (status, _, body) = call(addr, "GET", "/api/session", Some(&first), None).await;
    assert_eq!(
        (status, body["account"]["username"].as_str()),
        (200, Some("ann_01"))
    );
    for token in [None, Some("nonsense"), Some(&"0".repeat(64)[..])] {
        let (status, _, body) = call(addr, "GET", "/api/session", token, None).await;
        assert_eq!((status, body["error"].as_str()), (401, Some("NotLoggedIn")));
    }

    // Registration errors.
    let cases = [
        (creds("ANN_01", PW), 409, "UsernameTaken"),
        (creds("ann 01", PW), 400, "InvalidUsername"),
        (creds("", PW), 400, "InvalidUsername"),
        (creds("ben", "short"), 400, "PasswordTooShort"),
        (creds("ben", &"x".repeat(1025)), 400, "PasswordTooLong"),
        (Some(json!({ "username": "ben" })), 400, "BadRequest"),
        (Some(json!("ben")), 400, "BadRequest"),
    ];
    for (body, want_status, want_error) in cases {
        let (status, _, got) = call(addr, "POST", "/api/accounts", None, body).await;
        assert_eq!(
            (status, got["error"].as_str()),
            (want_status, Some(want_error))
        );
    }
    let huge = format!(r#"{{"username":"ben","password":"{}"}}"#, "x".repeat(5000));
    let (status, _, body) = http(addr, "POST", "/api/accounts", &[], Some(&huge)).await;
    assert_eq!(status, 400, "{body}");

    // Wrong password and unknown user look the same.
    let (status, body) = login(addr, "ann_01", "wrong password!").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("InvalidCredentials"))
    );
    let (status, body) = login(addr, "nobody", PW).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (401, Some("InvalidCredentials"))
    );

    // Several sessions; logging one out leaves the others.
    let (status, body) = login(addr, "ANN_01", PW).await;
    assert_eq!(status, 200);
    let second = body["token"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    let (status, _, _) = call(addr, "DELETE", "/api/session", Some(&second), None).await;
    assert_eq!(status, 204);
    let (status, _, _) = call(addr, "GET", "/api/session", Some(&second), None).await;
    assert_eq!(status, 401);
    let (status, _, _) = call(addr, "GET", "/api/session", Some(&first), None).await;
    assert_eq!(status, 200);
    // Logging out an unknown token is not an error.
    let (status, _, _) = call(addr, "DELETE", "/api/session", Some(&second), None).await;
    assert_eq!(status, 204);

    // Logging out everywhere.
    let (_, body) = login(addr, "ann_01", PW).await;
    let third = body["token"].as_str().unwrap().to_owned();
    let (status, _, _) = call(addr, "DELETE", "/api/sessions", None, None).await;
    assert_eq!(status, 401);
    let (status, _, _) = call(addr, "DELETE", "/api/sessions", Some(&first), None).await;
    assert_eq!(status, 204);
    for token in [&first, &third] {
        let (status, _, _) = call(addr, "GET", "/api/session", Some(token), None).await;
        assert_eq!(status, 401);
    }

    // Accounts and sessions are on disk.
    let (_, body) = login(addr, "ann_01", PW).await;
    let fourth = body["token"].as_str().unwrap().to_owned();
    server.stop().await;
    let server = Server::start(accounts_config(dir.clone())).await;
    let (status, _, body) = call(server.addr, "GET", "/api/session", Some(&fourth), None).await;
    assert_eq!(
        (status, body["account"]["username"].as_str()),
        (200, Some("ann_01"))
    );
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn login_attempts_are_rate_limited() {
    let dir = temp_dir("accounts-limit");
    let config = ServerConfig {
        auth_burst: 3,
        auth_refill: Duration::from_secs(60),
        client_ip_header: Some("X-Forwarded-For".into()),
        ..accounts_config(dir.clone())
    };
    let server = Server::start(config).await;
    let addr = server.addr;
    register(addr, "ann").await;
    for _ in 0..2 {
        assert_eq!(login(addr, "ann", "wrong password!").await.0, 401);
    }
    // Out of attempts: even the right password is refused for now.
    let (status, head, body) = call(addr, "POST", "/api/sessions", None, creds("ann", PW)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (429, Some("TooManyAttempts"))
    );
    let retry = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("retry-after: ")
                .map(str::to_owned)
        })
        .expect("Retry-After header");
    assert!((1..=60).contains(&retry.trim().parse::<u32>().unwrap()));
    // Registering draws from the same budget.
    let (status, _, _) = call(addr, "POST", "/api/accounts", None, creds("ben", PW)).await;
    assert_eq!(status, 429);
    // Checking a session is not limited.
    let (status, _, _) = call(addr, "GET", "/api/session", Some("x"), None).await;
    assert_eq!(status, 401);

    // Behind a trusted proxy, each forwarded client has its own budget; the
    // proxy appends the address it saw, so the last entry counts.
    let body = creds("ann", PW).unwrap().to_string();
    let send = |xff: &'static str| {
        let body = body.clone();
        async move {
            http(
                addr,
                "POST",
                "/api/sessions",
                &[("X-Forwarded-For", xff)],
                Some(&body),
            )
            .await
            .0
        }
    };
    assert_eq!(send("198.51.100.7").await, 200);
    for _ in 0..2 {
        assert_eq!(send("10.0.0.1, 198.51.100.7").await, 200);
    }
    assert_eq!(send("198.51.100.7").await, 429);
    assert_eq!(send("198.51.100.7, 198.51.100.8").await, 200);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn seats_belong_to_accounts() {
    let dir = temp_dir("accounts-seats");
    let config = accounts_config(dir.clone());
    let server = Server::start(config.clone()).await;
    let addr = server.addr;
    let alice = register(addr, "Alice").await;
    let bob = register(addr, "bob").await;
    let carol = register(addr, "carol").await;

    // The creator plays under the account's username, whatever name was sent.
    let host = Client::create_as(addr, 3, "Someone else", Some(&alice)).await;
    assert_eq!(host.seat, Some(0));
    assert_eq!(seat(&host, 0), ("alice".into(), true));
    let room = host.room_id.clone();
    let host_token = host.token.clone();

    // A guest may pick the same nickname but is not marked as registered.
    let guest = Client::join(addr, &room, "alice", None).await;
    assert_eq!(guest.seat, Some(1));
    assert_eq!(seat(&guest, 1), ("alice".into(), false));

    // A bad session is refused outright.
    let bad = Client::join_as(addr, &room, "x", None, Some(&"0".repeat(64))).await;
    assert_eq!(bad.errors, ["SessionExpired"]);
    assert_eq!(bad.seat, None);

    // Signing in on another device (no seat token) gets the seat back.
    let again = Client::join_as(addr, &room, "x", None, Some(&alice)).await;
    assert_eq!(again.seat, Some(0));
    assert_eq!(again.token, host_token);
    drop(again);

    let mut bob_client = Client::join_as(addr, &room, "B", None, Some(&bob)).await;
    assert_eq!(bob_client.seat, Some(2));
    bob_client
        .until(|c| {
            c.room
                .as_ref()
                .unwrap()
                .seats
                .iter()
                .all(|s| matches!(s, SeatView::Human { .. }))
        })
        .await;
    assert_eq!(seat(&bob_client, 2), ("bob".into(), true));

    // During the game: the account's seat from anywhere, others watch.
    let mut host = host;
    host.send(&ClientMsg::Start).await;
    host.until(|c| c.view.is_some()).await;
    let mut late = Client::join_as(addr, &room, "x", None, Some(&alice)).await;
    late.until(|c| c.view.is_some()).await;
    assert_eq!(late.seat, Some(0));
    let watcher = Client::join_as(addr, &room, "C", None, Some(&carol)).await;
    assert_eq!(watcher.seat, None);
    drop((host, guest, bob_client, late, watcher));
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.stop().await;

    // The binding is saved with the room.
    let server = Server::start(config).await;
    let mut back = Client::join_as(server.addr, &room, "x", None, Some(&alice)).await;
    back.until(|c| c.view.is_some()).await;
    assert_eq!(back.seat, Some(0));
    assert_eq!(seat(&back, 0), ("alice".into(), true));
    assert_eq!(seat(&back, 1), ("alice".into(), false));
    assert_eq!(back.view.as_ref().unwrap().players.len(), 3);
    server.stop().await;
    let _ = std::fs::remove_dir_all(&dir);
}
