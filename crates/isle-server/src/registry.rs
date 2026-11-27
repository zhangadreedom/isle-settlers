//! Directory of live rooms. Each room is an actor reached through a channel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use isle_proto::net::PublicRoomSummary;
use tokio::sync::{mpsc, oneshot, watch};

use crate::ServerConfig;
use crate::auth::Accounts;
use crate::ids;
use crate::room::{Attached, Outbox, Room, RoomCmd, RoomOptions};
use crate::store::{ReplayArchive, SeatAccount, Store};

pub type RoomSender = mpsc::Sender<RoomCmd>;

struct RoomEntry {
    tx: RoomSender,
    summary: watch::Receiver<Option<PublicRoomSummary>>,
}

pub struct Registry {
    config: ServerConfig,
    accounts: Option<Arc<Accounts>>,
    store: Option<Store>,
    replays: Arc<ReplayArchive>,
    rooms: Mutex<HashMap<String, RoomEntry>>,
    me: Weak<Registry>,
}

impl Registry {
    /// Creates the registry and restores rooms from the data directory.
    pub fn new(config: ServerConfig, accounts: Option<Arc<Accounts>>) -> Arc<Registry> {
        let store = config.data_dir.as_ref().map(|d| Store::open(d.clone()));
        let replays = Arc::new(ReplayArchive::new(config.data_dir.as_deref()));
        let registry = Arc::new_cyclic(|me| Registry {
            config,
            accounts,
            store,
            replays,
            rooms: Mutex::new(HashMap::new()),
            me: me.clone(),
        });
        if let Some(store) = &registry.store {
            let mut restored = 0;
            for record in store.load_all() {
                let id = record.id.clone();
                match Room::restore(record, &registry) {
                    Ok(room) => {
                        registry.spawn(id, room);
                        restored += 1;
                    }
                    Err(e) => tracing::warn!(room = %id, "could not restore room: {e}"),
                }
            }
            if restored > 0 {
                tracing::info!("restored {restored} room(s)");
            }
        }
        registry
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    pub(crate) fn store(&self) -> Option<&Store> {
        self.store.as_ref()
    }

    pub(crate) fn accounts(&self) -> Option<&Arc<Accounts>> {
        self.accounts.as_ref()
    }

    pub fn replays(&self) -> &Arc<ReplayArchive> {
        &self.replays
    }

    pub fn room_count(&self) -> usize {
        self.rooms.lock().expect("registry lock").len()
    }

    /// Active public rooms not yet finished, sorted with lobby (`!started`)
    /// rooms first and in-progress games after.
    pub fn public_rooms(&self) -> Vec<PublicRoomSummary> {
        let mut list: Vec<PublicRoomSummary> = self
            .rooms
            .lock()
            .expect("registry lock")
            .values()
            .filter_map(|entry| entry.summary.borrow().clone())
            .collect();
        list.sort_by(|a, b| a.started.cmp(&b.started).then_with(|| a.id.cmp(&b.id)));
        list
    }

    /// Creates a room and attaches the creator to seat 0.
    pub(crate) async fn create(
        &self,
        options: RoomOptions,
        name: String,
        account: Option<SeatAccount>,
        out: Outbox,
    ) -> Result<(RoomSender, Attached), String> {
        let players = options.players;
        if !(2..=options.scenario.max_players()).contains(&players)
            || !options.rules.is_valid_for(options.scenario, players)
        {
            return Err("BadMessage".into());
        }
        let id = {
            let rooms = self.rooms.lock().expect("registry lock");
            if rooms.len() >= self.config.max_rooms {
                return Err("TooManyRooms".into());
            }
            loop {
                let id = ids::room_id();
                if !rooms.contains_key(&id) {
                    break id;
                }
            }
        };
        let (room, token) =
            Room::new_lobby(id.clone(), options, name.clone(), account.clone(), self);
        let tx = self.spawn(id.clone(), room);
        tracing::info!(room = %id, players, "room created");
        self.attach(tx, name, Some(token), account, out).await
    }

    /// Joins (or rejoins with `token`) an existing room.
    pub(crate) async fn join(
        &self,
        room: &str,
        name: String,
        token: Option<String>,
        account: Option<SeatAccount>,
        out: Outbox,
    ) -> Result<(RoomSender, Attached), String> {
        let id = room.trim().to_ascii_uppercase();
        let tx = self
            .rooms
            .lock()
            .expect("registry lock")
            .get(&id)
            .map(|e| e.tx.clone())
            .ok_or("RoomNotFound")?;
        self.attach(tx, name, token, account, out).await
    }

    async fn attach(
        &self,
        tx: RoomSender,
        name: String,
        token: Option<String>,
        account: Option<SeatAccount>,
        out: Outbox,
    ) -> Result<(RoomSender, Attached), String> {
        let (reply, rx) = oneshot::channel();
        tx.send(RoomCmd::Attach {
            name,
            token,
            account,
            out,
            reply,
        })
        .await
        .map_err(|_| "RoomNotFound")?;
        let attached = rx.await.map_err(|_| "RoomNotFound")??;
        Ok((tx, attached))
    }

    /// Asks every room to save and stop; waits up to five seconds.
    pub async fn shutdown(&self) {
        let rooms: Vec<RoomSender> = self
            .rooms
            .lock()
            .expect("registry lock")
            .values()
            .map(|e| e.tx.clone())
            .collect();
        let mut waits = Vec::new();
        for tx in rooms {
            let (done, rx) = oneshot::channel();
            if tx.send(RoomCmd::Shutdown { done }).await.is_ok() {
                waits.push(rx);
            }
        }
        let all = async {
            for rx in waits {
                let _ = rx.await;
            }
        };
        if tokio::time::timeout(std::time::Duration::from_secs(5), all)
            .await
            .is_err()
        {
            tracing::warn!("some rooms did not stop in time");
        }
    }

    fn spawn(&self, id: String, room: Room) -> RoomSender {
        let (tx, rx) = mpsc::channel(256);
        let summary = room.subscribe_summary();
        self.rooms.lock().expect("registry lock").insert(
            id.clone(),
            RoomEntry {
                tx: tx.clone(),
                summary,
            },
        );
        let me = self.me.clone();
        tokio::spawn(async move {
            room.run(rx).await;
            if let Some(registry) = me.upgrade() {
                registry.rooms.lock().expect("registry lock").remove(&id);
            }
            tracing::info!(room = %id, "room closed");
        });
        tx
    }
}
