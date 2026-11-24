// Ranked play: leaderboard and player profiles (see isle_proto::ranking).

import { createSignal } from 'solid-js';
import type { Leaderboard, PlayerProfile } from '../generated/proto';
import { account, request, type ApiResult } from './account';

export const fetchLeaderboard = (limit: number, offset: number): Promise<ApiResult<Leaderboard>> =>
  request<Leaderboard>('GET', `/api/leaderboard?limit=${limit}&offset=${offset}`);

export const fetchProfile = (username: string): Promise<ApiResult<PlayerProfile>> =>
  request<PlayerProfile>('GET', `/api/players/${encodeURIComponent(username)}`);

const [mine, setMine] = createSignal<PlayerProfile | null>(null);

/** The signed-in player's profile, loaded by `refreshMyProfile`. */
export const myProfile = () => (mine()?.username === account()?.username ? mine() : null);

/** Reloads the signed-in player's profile (after logging in or a ranked game). */
export async function refreshMyProfile() {
  const me = account();
  if (!me) {
    setMine(null);
    return;
  }
  const r = await fetchProfile(me.username);
  if (r.ok && account()?.username === me.username) setMine(r.data);
}
