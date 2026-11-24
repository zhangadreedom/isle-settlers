import { createResource, createSignal, For, onMount, Show } from 'solid-js';
import type { LeaderboardEntry, PlayerProfile, RoomInfo, Tier } from '../generated/proto';
import { errorText, TIER_NAME } from '../game/text';
import { account } from '../net/account';
import { fetchLeaderboard, fetchProfile, myProfile, refreshMyProfile } from '../net/ranking';

const PAGE = 50;

/** Tier and rating; "定级" while placement games are still to play. */
export function TierBadge(props: { tier: Tier | null; rating: number }) {
  return (
    <span
      class={`tier tier-${props.tier?.toLowerCase() ?? 'placing'}`}
      title={props.tier ? `${TIER_NAME[props.tier]}段位 · 排位分 ${props.rating}` : `定级赛中 · 排位分 ${props.rating}`}
    >
      {props.tier ? TIER_NAME[props.tier] : '定级'} {props.rating}
    </span>
  );
}

/** Signed rating change, coloured. */
export function Delta(props: { before: number; after: number }) {
  const d = () => props.after - props.before;
  return (
    <span class="delta" classList={{ up: d() > 0, down: d() < 0 }}>
      {d() > 0 ? `+${d()}` : `${d()}`}
    </span>
  );
}

/** Rating changes of a finished online game, for the game-over dialog. */
export function RatingResult(props: { room: RoomInfo }) {
  const signedIn = () => props.room.seats.filter((s) => s.type === 'Human' && s.value.registered).length;
  return (
    <div class="rating-result">
      <h4>排位分</h4>
      <Show
        when={props.room.rating.length > 0}
        fallback={<p class="muted small">{signedIn() >= 2 ? '结算中…' : '登录玩家不足 2 人，本局不计排位分。'}</p>}
      >
        <ul>
          <For each={props.room.rating}>
            {(c) => (
              <li>
                <span class="grow">{c.username}</span>
                <span class="muted">
                  {c.before} → {c.after}
                </span>
                <Delta before={c.before} after={c.after} />
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
}

const winRate = (games: number, wins: number) => (games ? `${Math.round((wins / games) * 100)}%` : '—');

/** A player's rating, rank and recent ranked games with replay links. */
export function ProfileCard(props: { profile: PlayerProfile; title?: string }) {
  const p = () => props.profile;
  return (
    <div class="profile-card">
      <div class="profile-head">
        <b class="grow">{props.title ?? p().username}</b>
        <TierBadge tier={p().tier} rating={p().rating} />
      </div>
      <p class="muted small">
        <Show when={p().rank !== null} fallback={p().placementLeft > 0 ? `定级赛还差 ${p().placementLeft} 局 · ` : ''}>
          排行第 {p().rank} 名 ·{' '}
        </Show>
        {p().games} 局 · 胜 {p().wins} · 胜率 {winRate(p().games, p().wins)}
      </p>
      <Show when={p().recent.length > 0} fallback={<p class="muted small">还没有排位对局。在联机对战里选“排位”开房即可。</p>}>
        <ul class="recent">
          <For each={p().recent}>
            {(m) => (
              <li>
                <span class="muted">{new Date(m.finishedAt * 1000).toLocaleDateString()}</span>
                <span class="grow">
                  第 {m.place}/{m.seats} 名 · {m.points} 分
                </span>
                <Delta before={m.before} after={m.after} />
                <a href={`#/replay/${m.id}`} title="观看这局的回放">
                  回放
                </a>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </div>
  );
}

function ProfileView(props: { username: string; onBack: () => void }) {
  const [profile] = createResource(
    () => props.username,
    async (name) => {
      const r = await fetchProfile(name);
      if (!r.ok) throw new Error(r.error);
      return r.data;
    },
  );
  return (
    <div class="rankings">
      <button class="ghost small back" onClick={props.onBack}>
        ← 排行榜
      </button>
      <Show when={!profile.error} fallback={<p class="form-error">{errorText((profile.error as Error).message)}</p>}>
        <Show when={profile()} fallback={<p class="muted">加载中…</p>}>
          {(p) => <ProfileCard profile={p()} />}
        </Show>
      </Show>
    </div>
  );
}

/** Lobby tab: your standing, the leaderboard and player profiles. */
export function Rankings() {
  const [selected, setSelected] = createSignal<string | null>(null);
  const [entries, setEntries] = createSignal<LeaderboardEntry[]>([]);
  const [total, setTotal] = createSignal(0);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const load = async (offset: number) => {
    setLoading(true);
    const r = await fetchLeaderboard(PAGE, offset);
    setLoading(false);
    if (!r.ok) return setError(r.error);
    setError(null);
    setTotal(r.data.total);
    setEntries(offset === 0 ? r.data.entries : [...entries(), ...r.data.entries]);
  };
  onMount(() => {
    void load(0);
    void refreshMyProfile();
  });

  return (
    <Show when={selected()} fallback={
      <div class="rankings">
        <Show when={account()}>
          <Show when={myProfile()} fallback={<p class="muted small">加载我的排位…</p>}>
            {(p) => <ProfileCard profile={p()} title="我的排位" />}
          </Show>
        </Show>
        <Show when={!account()}>
          <p class="muted small">登录后在“排位”房间对局，打完定级赛就会出现在排行榜上。</p>
        </Show>
        <h4>排行榜</h4>
        <Show when={error()}>
          <p class="form-error">{errorText(error()!)}</p>
        </Show>
        <Show when={entries().length > 0} fallback={<p class="muted small">{loading() ? '加载中…' : '还没有完成定级赛的玩家。'}</p>}>
          <table class="leaderboard">
            <thead>
              <tr>
                <th>#</th>
                <th>玩家</th>
                <th>段位</th>
                <th class="num">局数</th>
                <th class="num">胜率</th>
              </tr>
            </thead>
            <tbody>
              <For each={entries()}>
                {(e) => (
                  <tr classList={{ me: e.username === account()?.username }}>
                    <td>{e.rank}</td>
                    <td>
                      <button class="link" onClick={() => setSelected(e.username)}>
                        {e.username}
                      </button>
                    </td>
                    <td>
                      <TierBadge tier={e.tier} rating={e.rating} />
                    </td>
                    <td class="num">{e.games}</td>
                    <td class="num">{winRate(e.games, e.wins)}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
          <Show when={entries().length < total()}>
            <button class="small" disabled={loading()} onClick={() => void load(entries().length)}>
              加载更多
            </button>
          </Show>
        </Show>
      </div>
    }>
      {(name) => <ProfileView username={name()} onBack={() => setSelected(null)} />}
    </Show>
  );
}
