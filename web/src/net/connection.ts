// WebSocket connection to the game server: handshake, automatic reconnects
// (rejoining with the seat token kept in localStorage) and message fan-out.

import { createSignal } from 'solid-js';
import type { BotLevel, ChatLine, ClientMsg, RoomInfo, ServerMsg, Snapshot, Update } from '../generated/proto';
import { forgetSession, sessionToken } from './account';

export type ConnStatus = 'connecting' | 'open' | 'reconnecting' | 'closed';

export type Hello =
  | { type: 'Create'; players: number; bots: BotLevel; name: string }
  | { type: 'Join'; room: string; name: string };

export const NAME_KEY = 'isle:name';
const seatKey = (room: string) => `isle:seat:${room.toUpperCase()}`;

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** Token saved for a room by an earlier visit, if any. */
export function savedToken(room: string): string | null {
  try {
    return storage()?.getItem(seatKey(room)) ?? null;
  } catch {
    return null;
  }
}

function saveToken(room: string, token: string | null) {
  try {
    if (token) storage()?.setItem(seatKey(room), token);
  } catch {
    // Private mode or blocked storage: reconnects still work within this tab.
  }
}

export function savedName(): string {
  try {
    return storage()?.getItem(NAME_KEY) ?? '';
  } catch {
    return '';
  }
}

export function saveName(name: string) {
  try {
    storage()?.setItem(NAME_KEY, name);
  } catch {
    // ignore
  }
}

/** Invite link for a room. */
export const inviteLink = (room: string) => `${location.origin}${location.pathname}#/r/${room}`;

function serverUrl(): string {
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${scheme}://${location.host}/api/ws`;
}

/** Errors during the handshake that retrying cannot fix. */
const FATAL = new Set(['RoomNotFound', 'TooManyRooms', 'BadMessage', 'SessionExpired', 'AccountsDisabled']);

export function createConnection(hello: Hello) {
  const [status, setStatus] = createSignal<ConnStatus>('connecting');
  const [room, setRoom] = createSignal<RoomInfo | null>(null);
  const [seat, setSeat] = createSignal<number | null>(null);
  const [fatal, setFatal] = createSignal<string | null>(null);
  const [snapshot, setSnapshot] = createSignal<Snapshot | null>(null);

  const onUpdate = new Set<(u: Update) => void>();
  const onSnapshot = new Set<(s: Snapshot) => void>();
  const onError = new Set<(code: string) => void>();
  const onChat = new Set<(c: ChatLine) => void>();

  let ws: WebSocket | null = null;
  let roomId = hello.type === 'Join' ? hello.room.toUpperCase() : null;
  let token: string | null = roomId ? savedToken(roomId) : null;
  let joined = false;
  let closed = false;
  let attempts = 0;
  let retryTimer: number | undefined;

  function open() {
    joined = false;
    const socket = new WebSocket(serverUrl());
    ws = socket;
    socket.onopen = () => {
      // Signed in: the seat is bound to the account (and found again by it
      // from other devices). A seat token alone is enough to come back.
      const session = token ? null : sessionToken();
      const first: ClientMsg = roomId
        ? { type: 'Join', value: { room: roomId, name: hello.name, token, session } }
        : { type: 'Create', value: { players: (hello as { players: number }).players, bots: (hello as { bots: BotLevel }).bots, name: hello.name, session } };
      socket.send(JSON.stringify(first));
    };
    socket.onmessage = (e) => handle(JSON.parse(e.data as string) as ServerMsg);
    socket.onclose = () => {
      if (ws !== socket) return;
      if (closed || fatal()) {
        setStatus('closed');
        return;
      }
      setStatus('reconnecting');
      // 0.5 s, 1 s, 2 s ... up to 8 s between attempts.
      const delay = Math.min(8000, 500 * 2 ** Math.min(attempts++, 4));
      retryTimer = window.setTimeout(open, delay);
    };
  }

  function handle(msg: ServerMsg) {
    switch (msg.type) {
      case 'Joined':
        joined = true;
        attempts = 0;
        roomId = msg.value.room;
        token = msg.value.token;
        saveToken(roomId, token);
        setSeat(msg.value.seat);
        setStatus('open');
        break;
      case 'Room':
        setRoom(msg.value);
        break;
      case 'Snapshot':
        setSnapshot(msg.value);
        onSnapshot.forEach((f) => f(msg.value));
        break;
      case 'Update':
        onUpdate.forEach((f) => f(msg.value));
        break;
      case 'Chat':
        onChat.forEach((f) => f(msg.value));
        break;
      case 'Error':
        if (!joined && FATAL.has(msg.value.code)) {
          if (msg.value.code === 'SessionExpired' || msg.value.code === 'AccountsDisabled') forgetSession();
          setFatal(msg.value.code);
          ws?.close();
        } else {
          onError.forEach((f) => f(msg.value.code));
        }
        break;
    }
  }

  function send(msg: ClientMsg): boolean {
    if (ws?.readyState !== WebSocket.OPEN || !joined) return false;
    ws.send(JSON.stringify(msg));
    return true;
  }

  function close() {
    closed = true;
    window.clearTimeout(retryTimer);
    ws?.close();
    setStatus('closed');
  }

  const subscribe =
    <T,>(set: Set<(v: T) => void>) =>
    (f: (v: T) => void) => {
      set.add(f);
      return () => set.delete(f);
    };

  open();

  return {
    status,
    room,
    seat,
    fatal,
    snapshot,
    roomId: () => roomId,
    send,
    close,
    onUpdate: subscribe(onUpdate),
    onSnapshot: subscribe(onSnapshot),
    onError: subscribe(onError),
    onChat: subscribe(onChat),
  };
}

export type Connection = ReturnType<typeof createConnection>;
