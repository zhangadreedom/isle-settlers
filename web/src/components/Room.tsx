import { createSignal, For, Show } from 'solid-js';
import { PLAYER_CSS } from '../board/palette';
import type { BotLevel, SeatView } from '../generated/proto';
import { errorText } from '../game/text';
import { account, accountsEnabled } from '../net/account';
import { inviteLink, savedName, saveName, type Connection } from '../net/connection';
import { AccountPanel, Verified } from './Account';
import { TierBadge } from './Ranking';

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

              <div class="room-mode">
                <Show
                  when={isHost()}
                  fallback={
                    <span class="mode-tag" classList={{ ranked: room().ranked }}>
                      {room().ranked ? '排位赛' : '休闲局'}
                    </span>
                  }
                >
                  <div class="seg small">
                    <button classList={{ on: room().ranked }} onClick={() => c.send({ type: 'SetRanked', value: { ranked: true } })}>
                      排位
                    </button>
                    <button classList={{ on: !room().ranked }} onClick={() => c.send({ type: 'SetRanked', value: { ranked: false } })}>
                      休闲
                    </button>
                  </div>
                </Show>
                <span class="muted small">{room().ranked ? '至少两名登录玩家时计入排位分' : '不计排位分'}</span>
                <Show when={room().scenario === 'Archipelago'}>
                  <span class="mode-tag sea" title="造船出海，登上新岛屿得奖励分，先到 12 分">
                    航海 · 群岛
                  </span>
                </Show>
                <Show when={room().scenario === 'FogIslands'}>
                  <span class="mode-tag sea" title="驶入迷雾翻开未知海域与岛屿，先到 12 分">
                    航海 · 迷雾
                  </span>
                </Show>
                <Show when={room().scenario === 'Citadels'}>
                  <span class="mode-tag citadel" title="商品、三系城邦升级与大都会、实体铁卫抵御黑帆舰队">
                    城邦与铁卫
                  </span>
                </Show>
                <Show when={room().rules.victoryPoints !== null}>
                  <span class="mode-tag">{room().rules.victoryPoints} 分获胜</span>
                </Show>
                <Show when={room().rules.friendlyRobber}>
                  <span class="mode-tag" title="强盗不封锁 ≤2 分玩家">友好强盗</span>
                </Show>
                <Show when={room().seats.length > 4 && room().rules.pairedTurns}>
                  <span class="mode-tag" title="隔 3 座位搭档执行副回合">双人搭档</span>
                </Show>
                <Show when={room().rules.turnTimerSecs > 0}>
                  <span class="mode-tag" title="超时自动代出一步">限时 {room().rules.turnTimerSecs}s</span>
                </Show>
              </div>

              <Show when={isHost()}>
                <details class="house-rules">
                  <summary>修改房间房规与限时</summary>
                  <div class="rules-box">
                    <div class="field">
                      <span>友好强盗</span>
                      <div class="seg small">
                        <button
                          classList={{ on: !room().rules.friendlyRobber }}
                          onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, friendlyRobber: false } } })}
                        >
                          标准
                        </button>
                        <button
                          classList={{ on: room().rules.friendlyRobber }}
                          onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, friendlyRobber: true } } })}
                        >
                          开启
                        </button>
                      </div>
                    </div>
                    <Show when={room().seats.length > 4}>
                      <div class="field">
                        <span>5–6人规则</span>
                        <div class="seg small">
                          <button
                            classList={{ on: !room().rules.pairedTurns }}
                            onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, pairedTurns: false } } })}
                          >
                            特殊建造
                          </button>
                          <button
                            classList={{ on: room().rules.pairedTurns }}
                            onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, pairedTurns: true } } })}
                          >
                            双人搭档
                          </button>
                        </div>
                      </div>
                    </Show>
                    <div class="field">
                      <span>每步限时</span>
                      <div class="seg small">
                        <For each={[0, 30, 60, 90]}>
                          {(secs) => (
                            <button
                              classList={{ on: room().rules.turnTimerSecs === secs }}
                              onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, turnTimerSecs: secs } } })}
                            >
                              {secs === 0 ? '不限' : `${secs}s`}
                            </button>
                          )}
                        </For>
                      </div>
                    </div>
                  </div>
                </details>
              </Show>

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
                        <Show when={s.type === 'Human' && s.value.rating}>
                          {(r) => <TierBadge tier={r().tier} rating={r().rating} />}
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
