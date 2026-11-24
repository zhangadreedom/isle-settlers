// Optional player accounts (see isle_proto::account). The session token is
// kept in localStorage and sent as a bearer token to the account API and with
// the first WebSocket message of a room, which binds the seat to the account.

import { createSignal } from 'solid-js';
import type { AccountInfo, ApiError, SessionGrant, SessionInfo } from '../generated/proto';

/** Registration rules, as enforced by the server (isle-server `accounts`). */
export const USERNAME_RULE = /^[A-Za-z0-9_-]{1,32}$/;
export const MIN_PASSWORD_BYTES = 12;
export const MAX_PASSWORD_BYTES = 1024;

const SESSION_KEY = 'isle:session';

interface Saved {
  token: string;
  account: AccountInfo;
  expiresAt: number;
}

function load(): Saved | null {
  try {
    const raw = window.localStorage.getItem(SESSION_KEY);
    if (!raw) return null;
    const s = JSON.parse(raw) as Partial<Saved>;
    const ok =
      typeof s.token === 'string' &&
      typeof s.account?.username === 'string' &&
      typeof s.expiresAt === 'number' &&
      s.expiresAt * 1000 > Date.now();
    return ok ? (s as Saved) : null;
  } catch {
    return null;
  }
}

function store(s: Saved | null) {
  try {
    if (s) window.localStorage.setItem(SESSION_KEY, JSON.stringify(s));
    else window.localStorage.removeItem(SESSION_KEY);
  } catch {
    // Blocked storage: the login lasts as long as the page.
  }
}

let saved: Saved | null = load();
const [enabled, setEnabled] = createSignal(false);
const [current, setCurrent] = createSignal<AccountInfo | null>(saved?.account ?? null);

function set(s: Saved | null) {
  saved = s;
  store(s);
  setCurrent(s?.account ?? null);
}

/** Whether the server offers accounts (it needs a data directory). */
export const accountsEnabled = enabled;

/** The signed-in account, or null for guests. */
export const account = () => (enabled() ? current() : null);

/** Token to send with `Create` / `Join`, if signed in. */
export function sessionToken(): string | null {
  if (!enabled() || !saved) return null;
  return saved.expiresAt * 1000 > Date.now() ? saved.token : null;
}

/** Drops the local session (the server refused it). */
export function forgetSession() {
  set(null);
}

export type ApiResult<T> = { ok: true; data: T } | { ok: false; error: string };

/** JSON request to the server's API; failures carry an `ApiError` code. */
export async function request<T>(method: string, path: string, body?: unknown, token?: string): Promise<ApiResult<T>> {
  try {
    const headers: Record<string, string> = {};
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (token) headers.Authorization = `Bearer ${token}`;
    const res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    if (res.status === 204) return { ok: true, data: undefined as T };
    const json = (await res.json().catch(() => null)) as T | ApiError | null;
    if (res.ok && json) return { ok: true, data: json as T };
    return { ok: false, error: (json as ApiError | null)?.error ?? `Http${res.status}` };
  } catch {
    return { ok: false, error: 'NetworkError' };
  }
}

/**
 * Asks the server whether it offers accounts and checks a saved session.
 * Until it answers, the stored account is shown optimistically.
 */
export async function initAccounts() {
  try {
    const res = await fetch('/api/health');
    const health = (await res.json()) as { accounts?: boolean };
    setEnabled(health.accounts === true);
  } catch {
    setEnabled(false);
  }
  if (!enabled() || !saved) return;
  const token = saved.token;
  const r = await request<SessionInfo>('GET', '/api/session', undefined, token);
  if (saved?.token !== token) return; // logged in or out meanwhile
  if (r.ok) set({ token, account: r.data.account, expiresAt: r.data.expiresAt });
  else if (r.error === 'NotLoggedIn') set(null);
}

/** Error code for local checks before registering, or null. */
export function checkRegistration(username: string, password: string): string | null {
  if (!USERNAME_RULE.test(username.trim())) return 'InvalidUsername';
  const bytes = new TextEncoder().encode(password).length;
  if (bytes < MIN_PASSWORD_BYTES) return 'PasswordTooShort';
  if (bytes > MAX_PASSWORD_BYTES) return 'PasswordTooLong';
  return null;
}

async function signIn(path: string, username: string, password: string): Promise<string | null> {
  const r = await request<SessionGrant>('POST', path, { username: username.trim(), password });
  if (!r.ok) return r.error;
  set({ token: r.data.token, account: r.data.account, expiresAt: r.data.expiresAt });
  return null;
}

/** Creates an account and signs in. Returns an error code or null. */
export const register = (username: string, password: string) => signIn('/api/accounts', username, password);

/** Signs in. Returns an error code or null. */
export const login = (username: string, password: string) => signIn('/api/sessions', username, password);

/** Signs out here; seats already taken stay yours through their seat token. */
export async function logout() {
  const token = saved?.token;
  set(null);
  if (token) await request('DELETE', '/api/session', undefined, token);
}

/** Ends every session of the account, on all devices. */
export async function logoutEverywhere(): Promise<string | null> {
  const token = saved?.token;
  if (!token) return null;
  const r = await request('DELETE', '/api/sessions', undefined, token);
  if (r.ok || r.error === 'NotLoggedIn') {
    set(null);
    return null;
  }
  return r.error;
}

// Follow logins and logouts in other tabs.
window.addEventListener('storage', (e) => {
  if (e.key !== SESSION_KEY) return;
  saved = load();
  setCurrent(saved?.account ?? null);
});
