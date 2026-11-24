import { createSignal, For, Show } from 'solid-js';
import { PLAYER_CSS } from '../board/palette';
import type { BotLevel, SeatView } from '../generated/proto';
import { errorText } from '../game/text';
import { account, accountsEnabled } from '../net/account';
import { inviteLink, savedName, saveName, type Connection } from '../net/connection';
import { AccountPanel, Verified } from './Account';

/** Name prompt shown when opening an invite link for the first time. */
export function JoinPrompt(props: { room: string; onJoin: (name: string) => void; onCancel: () => void }) {
  const [name, setName] = createSignal(savedName());
  const submit = (e: Event) => {
    e.preventDefault();
    const signedIn = account();
    if (signedIn) return props.onJoin(signedIn.username);
    const n = name().trim() || '玩家';
    saveName(n);
    props.onJoin(n);
  };
  return (
    <div class="lobby-wrap">
      <div class="lobby-stack">
        <form class="panel lobby" onSubmit={submit}>
          <h1>加入房间</h1>
          <p class="muted">
            房间 <code class="seed">{props.room}</code>
          </p>
          <Show
            when={account()}
            fallback={
              <div class="field">
                <span>昵称</span>
                <input
                  class="grow"
                  value={name()}
                  maxLength={16}
                  placeholder="你的名字"
                  onInput={(e) => setName(e.currentTarget.value)}
                  aria-label="昵称"
                  autofocus
                />
              </div>
            }
          >
            {(a) => <p class="muted">以账号 <b>{a().username}</b> 的身份加入。</p>}
          </Show>
          <div class="row lobby-actions">
            <button class="primary big" type="submit">
              加入
            </button>
            <button type="button" onClick={props.onCancel}>
              返回
            </button>
          </div>
        </form>
        <Show when={accountsEnabled()}>
          <div class="panel lobby">
            <AccountPanel />
          </div>
        </Show>
      </div>
    </div>
  );
}

const LEVEL_NAME: Record<BotLevel, string> = { Easy: 'AI · 简单', Normal: 'AI · 普通', Hard: 'AI · 困难' };

function seatLabel(s: SeatView): string {
  switch (s.type) {
    case 'Open':
      return '空位 · 等待加入';
    case 'Bot':
      return LEVEL_NAME[s.value.level];
    case 'Human':
      return s.value.name;
  }
}

/** Waiting room before the game starts. */
export function RoomLobby(props: { conn: Connection; onLeave: () => void }) {
  const c = props.conn;
  const [copied, setCopied] = createSignal(false);
  const isHost = () => c.room() !== null && c.seat() === c.room()!.host;

  const copy = async () => {
    const link = inviteLink(c.roomId() ?? '');
    try {
      await navigator.clipboard.writeText(link);
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    } catch {
      window.prompt('复制邀请链接：', link);
    }
  };

  return (
    <div class="lobby-wrap">
      <div class="panel lobby room">
        <Show
          when={c.room()}
          fallback={
            <>
              <h1>{c.fatal() ? '无法加入' : '连接中…'}</h1>
              <Show when={c.fatal()}>
                <p class="muted">{errorText(c.fatal()!)}</p>
              </Show>
              <div class="row lobby-actions">
                <button onClick={props.onLeave}>返回</button>
              </div>
            </>
          }
        >
          {(room) => (
            <>
              <h1>
                房间 <code class="seed big">{room().id}</code>
              </h1>
              <div class="invite">
                <input readOnly value={inviteLink(room().id)} aria-label="邀请链接" onFocus={(e) => e.currentTarget.select()} />
                <button class="primary" onClick={copy}>
                  {copied() ? '已复制' : '复制邀请链接'}
                </button>
              </div>

              <ol class="seats">
                <For each={room().seats}>
                  {(s, i) => (
                    <li classList={{ me: i() === c.seat() }}>
                      <span class="dot" style={{ background: PLAYER_CSS[i()] }} />
                      <span class="seat-label">
                        {seatLabel(s)}
                        <Show when={s.type === 'Human' && s.value.registered}>
                          <Verified />
                        </Show>
                        {i() === c.seat() ? '（你）' : ''}
                        {i() === room().host ? ' · 房主' : ''}
                      </span>
                      <Show when={s.type === 'Human' && !(s as { value: { connected: boolean } }).value.connected}>
                        <span class="presence off">离线</span>
                      </Show>
                      <Show when={isHost() && s.type !== 'Human'}>
                        <div class="seg small">
                          <button classList={{ on: s.type === 'Open' }} onClick={() => c.send({ type: 'SetSeat', value: { seat: i(), bot: null } })}>
                            空位
                          </button>
                          <button
                            classList={{ on: s.type === 'Bot' && s.value.level === 'Easy' }}
                            onClick={() => c.send({ type: 'SetSeat', value: { seat: i(), bot: 'Easy' } })}
                          >
                            简单 AI
                          </button>
                          <button
                            classList={{ on: s.type === 'Bot' && s.value.level === 'Normal' }}
                            onClick={() => c.send({ type: 'SetSeat', value: { seat: i(), bot: 'Normal' } })}
                          >
                            普通 AI
                          </button>
                          <button
                            classList={{ on: s.type === 'Bot' && s.value.level === 'Hard' }}
                            onClick={() => c.send({ type: 'SetSeat', value: { seat: i(), bot: 'Hard' } })}
                          >
                            困难 AI
                          </button>
                        </div>
                      </Show>
                    </li>
                  )}
                </For>
              </ol>
              <Show when={room().spectators > 0}>
                <p class="muted small">{room().spectators} 名观众</p>
              </Show>
              <Show when={c.seat() === null}>
                <p class="muted">座位已满，你将以观众身份观看。</p>
              </Show>

              <div class="row lobby-actions">
                <Show
                  when={isHost()}
                  fallback={<p class="muted">等待房主开始游戏…</p>}
                >
                  <button class="primary big" onClick={() => c.send({ type: 'Start' })}>
                    开始游戏
                  </button>
                  <Show when={room().seats.some((s) => s.type === 'Open')}>
                    <span class="muted small">空位将由 AI 补上</span>
                  </Show>
                </Show>
                <button onClick={props.onLeave}>离开</button>
              </div>
            </>
          )}
        </Show>
      </div>
    </div>
  );
}
