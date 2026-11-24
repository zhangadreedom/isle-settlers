//! WebSocket connections. The first message picks the room (create or join);
//! afterwards client messages are forwarded to the room actor and a writer
//! task drains the connection's outbox into the socket.

use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::stream::SplitStream;
use futures_util::{SinkExt, StreamExt};
use isle_proto::net::{ClientMsg, ServerMsg};
use tokio::sync::mpsc;
use tokio::time::{Instant, interval, timeout};

use crate::AppState;
use crate::room::{Out, RoomCmd, RoomOptions};
use crate::store::SeatAccount;

/// Largest accepted client frame; real messages are a few hundred bytes.
const MAX_MESSAGE: usize = 16 * 1024;
/// Frames that may queue up for a slow client before it is dropped.
const OUTBOX: usize = 512;
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
const PING_EVERY: Duration = Duration::from_secs(20);
/// Connections silent for this long (no pong, no message) are closed.
const SILENCE_LIMIT: Duration = Duration::from_secs(50);

pub async fn upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.max_message_size(MAX_MESSAGE)
        .max_frame_size(MAX_MESSAGE)
        .on_upgrade(move |socket| serve(socket, state))
}

fn error_frame(code: &str) -> Out {
    let msg = ServerMsg::Error {
        code: code.to_owned(),
    };
    Out::Text(serde_json::to_string(&msg).expect("serializable").into())
}

/// Next text frame parsed as a client message; `None` when the socket ends.
async fn next_message(stream: &mut SplitStream<WebSocket>) -> Option<Result<ClientMsg, ()>> {
    loop {
        match stream.next().await? {
            Ok(Message::Text(text)) => return Some(serde_json::from_str(&text).map_err(|_| ())),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

/// The account behind the session token sent with `Create` / `Join`.
async fn seat_account(
    state: &AppState,
    session: Option<String>,
) -> Result<Option<SeatAccount>, String> {
    let Some(token) = session else {
        return Ok(None);
    };
    let Some(accounts) = &state.accounts else {
        return Err("AccountsDisabled".into());
    };
    match accounts.seat_account(token).await {
        Ok(Some(account)) => Ok(Some(account)),
        Ok(None) => Err("SessionExpired".into()),
        Err(e) => {
            tracing::error!("session check failed: {e}");
            Err("ServerError".into())
        }
    }
}

async fn serve(socket: WebSocket, state: AppState) {
    let (mut sink, mut stream) = socket.split();
    let (out, mut outbox) = mpsc::channel::<Out>(OUTBOX);

    let writer = tokio::spawn(async move {
        while let Some(frame) = outbox.recv().await {
            let msg = match frame {
                Out::Text(text) => Message::Text(text.as_ref().into()),
                Out::Ping => Message::Ping(Default::default()),
            };
            if sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    let joined = match timeout(HELLO_TIMEOUT, next_message(&mut stream)).await {
        Ok(Some(Ok(ClientMsg::Create {
            players,
            bots,
            name,
            ranked,
            scenario,
            session,
        }))) => match seat_account(&state, session).await {
            Ok(account) => {
                let options = RoomOptions {
                    players,
                    bots,
                    ranked,
                    scenario,
                };
                state
                    .registry
                    .create(options, name, account, out.clone())
                    .await
            }
            Err(code) => Err(code),
        },
        Ok(Some(Ok(ClientMsg::Join {
            room,
            name,
            token,
            session,
        }))) => match seat_account(&state, session).await {
            Ok(account) => {
                state
                    .registry
                    .join(&room, name, token, account, out.clone())
                    .await
            }
            Err(code) => Err(code),
        },
        Ok(Some(_)) => Err("BadMessage".to_owned()),
        Ok(None) | Err(_) => Err(String::new()),
    };
    let (room, attached) = match joined {
        Ok(ok) => ok,
        Err(code) => {
            if !code.is_empty() {
                let _ = out.send(error_frame(&code)).await;
            }
            drop(out);
            let _ = writer.await;
            return;
        }
    };
    let conn = attached.conn;

    let mut ping = interval(PING_EVERY);
    ping.tick().await;
    let mut last_seen = Instant::now();
    loop {
        tokio::select! {
            msg = stream.next() => {
                let Some(Ok(msg)) = msg else { break };
                last_seen = Instant::now();
                match msg {
                    Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                        Ok(msg) => {
                            if room.send(RoomCmd::Msg { conn, msg }).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = out.try_send(error_frame("BadMessage"));
                        }
                    },
                    Message::Close(_) => break,
                    // Pongs refresh `last_seen`; pings are answered by axum.
                    _ => {}
                }
            }
            _ = ping.tick() => {
                if last_seen.elapsed() > SILENCE_LIMIT || out.try_send(Out::Ping).is_err() {
                    break;
                }
            }
        }
    }

    let _ = room.send(RoomCmd::Detach { conn }).await;
    drop(out);
    // The writer stops once the room has dropped its sender too.
    let mut writer = writer;
    if timeout(Duration::from_secs(5), &mut writer).await.is_err() {
        writer.abort();
    }
}
