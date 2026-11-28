import { createSignal, For, onCleanup, Show } from 'solid-js';
import type { BoardRenderer } from '../board/BoardRenderer';
import { PLAYER_CSS } from '../board/palette';
import type { BotLevel, ChatLine, SeatView } from '../generated/proto';
import { errorText } from '../game/text';
import { account, accountsEnabled } from '../net/account';
import { inviteLink, savedName, saveName, type Connection } from '../net/connection';
import { AccountPanel, Verified } from './Account';
import { SoundToggle, ThemeToggle } from './Hud';
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
export function RoomLobby(props: {
  conn: Connection;
  renderer?: BoardRenderer;
  onLeave: () => void;
  onOpenRules: () => void;
  onOpenWorkshop: () => void;
}) {
  const c = props.conn;
  const [copied, setCopied] = createSignal(false);
  const [swapFrom, setSwapFrom] = createSignal<number | null>(null);
  const [chatLines, setChatLines] = createSignal<ChatLine[]>([]);
  const [chatText, setChatText] = createSignal('');
  const [roomToast, setRoomToast] = createSignal<string | null>(null);
  const isHost = () => c.room() !== null && c.seat() === c.room()!.host;

  onCleanup(
    c.onChat((line) => {
      setChatLines((old) => [...old, line].slice(-40));
    }),
  );
  onCleanup(
    c.onError((code) => {
      setRoomToast(errorText(code));
      window.setTimeout(() => setRoomToast(null), 2400);
    }),
  );

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
              <div class="lobby-head">
                <h1>
                  房间 <code class="seed big">{room().id}</code>
                </h1>
                <div class="row lobby-tools">
                  <Show when={isHost()}>
                    <button class="ghost small" onClick={props.onOpenWorkshop}>
                      地图工坊
                    </button>
                  </Show>
                  <button class="ghost small" onClick={props.onOpenRules}>
                    规则
                  </button>
                  <ThemeToggle renderer={props.renderer} class="ghost small" />
                  <SoundToggle class="ghost small" />
                </div>
              </div>
              <Show when={roomToast()}>
                {(msg) => <p class="room-notice">{msg()}</p>}
              </Show>
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
                  <div class="seg small">
                    <button
                      classList={{ on: room().isPublic }}
                      onClick={() => c.send({ type: 'SetPublic', value: { is_public: true } })}
                      title="显示在大厅公开房间列表"
                    >
                      公开
                    </button>
                    <button
                      classList={{ on: !room().isPublic }}
                      onClick={() => c.send({ type: 'SetPublic', value: { is_public: false } })}
                      title="仅凭房间码或邀请链接加入"
                    >
                      私密
                    </button>
                  </div>
                </Show>
                <Show when={!isHost()}>
                  <span class="mode-tag">{room().isPublic ? '公开房' : '私密房'}</span>
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
                <Show when={room().scenario === 'Fishermen'}>
                  <span class="mode-tag sea" title="沿海渔场与荒漠内陆湖产鱼，兑换奇策与转交破旧皮靴">
                    渔场与丰年
                  </span>
                </Show>
                <Show when={room().rules.victoryPoints !== null}>
                  <span class="mode-tag">{room().rules.victoryPoints} 分获胜</span>
                </Show>
                <Show when={room().rules.friendlyRobber}>
                  <span class="mode-tag" title="强盗不封锁 ≤2 分玩家">友好强盗</span>
                </Show>
                <Show when={room().rules.eventDeck}>
                  <span class="mode-tag" title="使用 36 张理论概率点数牌发点">均衡牌组</span>
                </Show>
                <Show when={room().seats.length > 4 && room().rules.pairedTurns}>
                  <span class="mode-tag" title="隔 3 座位搭档执行副回合">双人搭档</span>
                </Show>
                <Show when={room().rules.turnTimerSecs > 0}>
                  <span class="mode-tag" title="超时自动代出一步">限时 {room().rules.turnTimerSecs}s</span>
                </Show>
                <Show when={room().rules.customMap}>
                  <span class="mode-tag" title="本局使用 DIY 自定义地图">DIY 地图</span>
                  <Show when={isHost()}>
                    <button
                      class="ghost small"
                      onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, customMap: null } } })}
                    >
                      恢复随机图
                    </button>
                  </Show>
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
                    <div class="field">
                      <span>点数产生</span>
                      <div class="seg small">
                        <button
                          classList={{ on: !room().rules.eventDeck }}
                          onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, eventDeck: false } } })}
                        >
                          标准双骰
                        </button>
                        <button
                          classList={{ on: room().rules.eventDeck }}
                          onClick={() => c.send({ type: 'SetRules', value: { rules: { ...room().rules, eventDeck: true } } })}
                        >
                          36张均衡牌组
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
                      <Show when={isHost()}>
                        <div class="row seat-ops">
                          <Show
                            when={swapFrom() !== null}
                            fallback={
                              <button class="ghost small" onClick={() => setSwapFrom(i())} title="调整座位先后手顺序">
                                换座
                              </button>
                            }
                          >
                            <Show
                              when={swapFrom() === i()}
                              fallback={
                                <button
                                  class="primary small"
                                  onClick={() => {
                                    const a = swapFrom()!;
                                    setSwapFrom(null);
                                    c.send({ type: 'SwapSeats', value: { a, b: i() } });
                                  }}
                                >
                                  换到此座
                                </button>
                              }
                            >
                              <button class="ghost small" onClick={() => setSwapFrom(null)}>
                                取消
                              </button>
                            </Show>
                          </Show>
                          <Show when={s.type === 'Human' && i() !== room().host}>
                            <button
                              class="ghost small danger-text"
                              onClick={() => c.send({ type: 'KickSeat', value: { seat: i() } })}
                              title="将该玩家移为观众并腾出座位"
                            >
                              踢出
                            </button>
                          </Show>
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
                <p class="muted">你当前为观众身份。</p>
              </Show>

              <div class="room-chat">
                <Show when={chatLines().length > 0}>
                  <ul class="room-chat-list">
                    <For each={chatLines()}>
                      {(line) => (
                        <li>
                          <b>{line.name}：</b>
                          <span>{line.text}</span>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
                <form
                  class="chat-form room-chat-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (chatText().trim()) {
                      c.send({ type: 'Chat', value: { text: chatText() } });
                      setChatText('');
                    }
                  }}
                >
                  <input
                    class="grow"
                    value={chatText()}
                    maxLength={200}
                    placeholder="房间准备聊天…"
                    onInput={(e) => setChatText(e.currentTarget.value)}
                    aria-label="房间聊天"
                  />
                  <button type="submit">发送</button>
                </form>
              </div>

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
