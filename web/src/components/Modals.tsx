import { createEffect, createSignal, For, Show } from 'solid-js';
import type { Game } from '../game/controller';
import type { CityTrack, CommoditySet, ReplayRecord, ResourceSet, RoomInfo, TradeGood, TradeResponse } from '../generated/proto';
import { refreshMyProfile } from '../net/ranking';
import { RatingResult } from './Ranking';
import {
  CITY_TRACKS,
  COMMODITIES,
  COMMODITY_NAME,
  RESOURCES,
  RESOURCE_NAME,
  TRACK_COMMODITY,
  TRACK_NAME,
  TRACK_PERK,
  setTotal,
} from '../game/text';
import { CommIcon, Modal, ResIcon, SeatDot, SetView, Stepper } from './bits';
import { EMBEDDED } from '../embed';
import { downloadReplay } from './Hud';

const empty = (): ResourceSet => [0, 0, 0, 0, 0];
const emptyComm = (): CommoditySet => [0, 0, 0];
const withCount = (s: ResourceSet, i: number, v: number): ResourceSet => s.map((x, j) => (j === i ? v : x)) as ResourceSet;
const withCommCount = (s: CommoditySet, i: number, v: number): CommoditySet => s.map((x, j) => (j === i ? v : x)) as CommoditySet;

export function DiscardModal(props: { game: Game }) {
  const g = props.game;
  const need = () => (g.me === null ? 0 : g.view().pendingDiscard[g.me]);
  const hand = () => g.view().me?.hand ?? empty();
  const comm = () => g.view().me?.commodities ?? emptyComm();
  const [pick, setPick] = createSignal<ResourceSet>(empty());
  const [pickComm, setPickComm] = createSignal<CommoditySet>(emptyComm());
  const total = () => setTotal(pick()) + setTotal(pickComm());
  const submit = () => {
    const ok = g.citadels ? g.actDiscardCitadel(pick(), pickComm()) : g.actSet('Discard', pick());
    if (ok) {
      setPick(empty());
      setPickComm(emptyComm());
    }
  };
  return (
    <Modal title={`弃掉 ${need()} 张牌`}>
      <p class="muted">掷出了 7，你的手牌超过 7 张，需要弃掉一半（向下取整）。</p>
      <div class="pickers">
        <For each={RESOURCES}>
          {(_, i) => (
            <Stepper
              label={<ResIcon r={i()} size="md" />}
              value={pick()[i()]}
              max={Math.min(hand()[i()], pick()[i()] + need() - total())}
              onChange={(v) => setPick(withCount(pick(), i(), v))}
            />
          )}
        </For>
        <Show when={g.citadels}>
          <For each={COMMODITIES}>
            {(_, i) => (
              <Stepper
                label={<CommIcon c={i()} size="md" />}
                value={pickComm()[i()]}
                max={Math.min(comm()[i()], pickComm()[i()] + need() - total())}
                onChange={(v) => setPickComm(withCommCount(pickComm(), i(), v))}
              />
            )}
          </For>
        </Show>
      </div>
      <footer>
        <span class="muted">
          已选 {total()} / {need()}
        </span>
        <button class="primary" disabled={total() !== need()} onClick={submit}>
          确认弃牌
        </button>
      </footer>
    </Modal>
  );
}

export function VictimModal(props: { game: Game }) {
  const g = props.game;
  return (
    <Show when={g.victimChoice()}>
      {(c) => (
        <Modal title="抢谁的牌？" onClose={g.clearVictimChoice}>
          <div class="choices">
            <For each={c().victims}>
              {(q) => (
                <button
                  class="choice"
                  onClick={() => {
                    const kv = c().knightVertex;
                    if (kv !== undefined) {
                      g.act((a) => a.type === 'ChaseRobber' && a.value.vertex === kv && a.value.tile === c().tile && a.value.victim === q);
                    } else {
                      g.act((a) => a.type === 'MoveRobber' && a.value.tile === c().tile && a.value.victim === q);
                    }
                    g.clearVictimChoice();
                  }}
                >
                  <SeatDot seat={q} /> {g.name(q)}
                  <span class="muted">{g.view().players[q].resourceCount + g.view().players[q].commodityCount} 张牌</span>
                </button>
              )}
            </For>
          </div>
        </Modal>
      )}
    </Show>
  );
}

export function YearOfPlentyModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  const [pick, setPick] = createSignal<ResourceSet>(empty());
  const want = () => Math.min(2, setTotal(g.view().bank));
  return (
    <Modal title="丰收：从银行拿 2 张资源" onClose={props.onClose}>
      <div class="pickers">
        <For each={RESOURCES}>
          {(_, i) => (
            <Stepper
              label={<ResIcon r={i()} size="md" />}
              value={pick()[i()]}
              max={Math.min(g.view().bank[i()], pick()[i()] + want() - setTotal(pick()))}
              onChange={(v) => setPick(withCount(pick(), i(), v))}
            />
          )}
        </For>
      </div>
      <footer>
        <button
          class="primary"
          disabled={setTotal(pick()) !== want()}
          onClick={() => g.actSet('PlayYearOfPlenty', pick()) && props.onClose()}
        >
          拿取
        </button>
      </footer>
    </Modal>
  );
}

/** Seafaring: pick the resources a gold field produced. */
export function GoldModal(props: { game: Game }) {
  const g = props.game;
  const owed = () => (g.me === null ? 0 : (g.view().pendingGold[g.me] ?? 0));
  const want = () => Math.min(owed(), setTotal(g.view().bank));
  const [pick, setPick] = createSignal<ResourceSet>(empty());
  return (
    <Modal title={`金矿：任选 ${want()} 张资源`}>
      <p class="muted">你的村庄或城市挨着产出的金矿，可以从银行任选资源（银行不够时有多少拿多少）。</p>
      <div class="pickers">
        <For each={RESOURCES}>
          {(_, i) => (
            <Stepper
              label={<ResIcon r={i()} size="md" />}
              value={pick()[i()]}
              max={Math.min(g.view().bank[i()], pick()[i()] + want() - setTotal(pick()))}
              onChange={(v) => setPick(withCount(pick(), i(), v))}
            />
          )}
        </For>
      </div>
      <footer>
        <span class="muted">
          已选 {setTotal(pick())} / {want()}
        </span>
        <button class="primary" disabled={setTotal(pick()) !== want()} onClick={() => g.actSet('ChooseGold', pick()) && setPick(empty())}>
          拿取
        </button>
      </footer>
    </Modal>
  );
}

/** Seafaring: a coastal edge takes a road or a ship. */
export function RouteChoiceModal(props: { game: Game }) {
  const g = props.game;
  return (
    <Show when={g.routeChoice() !== null}>
      <Modal title="修路还是造船？" onClose={g.clearRouteChoice}>
        <p class="muted">这条边在海岸上：道路连向陆地，船可以继续驶向海上。</p>
        <div class="choices row">
          <button
            class="choice"
            onClick={() => {
              const e = g.routeChoice();
              g.clearRouteChoice();
              g.act((a) => a.type === 'BuildRoad' && a.value === e);
            }}
          >
            道路
          </button>
          <button
            class="choice"
            onClick={() => {
              const e = g.routeChoice();
              g.clearRouteChoice();
              g.act((a) => a.type === 'BuildShip' && a.value === e);
            }}
          >
            船
          </button>
        </div>
      </Modal>
    </Show>
  );
}

export function MonopolyModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  return (
    <Modal title="垄断：选择一种资源" onClose={props.onClose}>
      <p class="muted">所有对手都要交出手中全部这种资源。</p>
      <div class="choices row">
        <For each={RESOURCES}>
          {(r, i) => (
            <button class="choice" onClick={() => g.act((a) => a.type === 'PlayMonopoly' && a.value === r) && props.onClose()}>
              <ResIcon r={i()} size="md" /> {RESOURCE_NAME[r]}
            </button>
          )}
        </For>
      </div>
    </Modal>
  );
}

const RESPONSE_TEXT: Record<TradeResponse, string> = { Pending: '考虑中…', Accepted: '接受', Rejected: '拒绝' };

export function TradeModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  const isPaired = () => g.view().phase.type === 'PairedTurn';
  const [tab, setTab] = createSignal<'bank' | 'players'>(g.view().trade && !isPaired() ? 'players' : 'bank');
  const [give, setGive] = createSignal<ResourceSet>(empty());
  const [get, setGet] = createSignal<ResourceSet>(empty());
  const [bankGive, setBankGive] = createSignal<TradeGood | null>(null);
  const me = () => g.view().me!;
  const offer = () => g.view().trade;

  const sameGood = (a: TradeGood, b: TradeGood) => a.type === b.type && a.value === b.value;
  const canGetGood = (want: TradeGood) => {
    const giveGood = bankGive();
    if (!giveGood) return false;
    if (giveGood.type === 'Resource' && want.type === 'Resource') {
      return g.legal().actions.some((a) => a.type === 'MaritimeTrade' && a.value.give === giveGood.value && a.value.get === want.value);
    }
    return g
      .legal()
      .actions.some((a) => a.type === 'MaritimeTradeGood' && sameGood(a.value.give, giveGood) && sameGood(a.value.get, want));
  };
  const doBankTrade = (want: TradeGood) => {
    const giveGood = bankGive();
    if (!giveGood) return;
    if (giveGood.type === 'Resource' && want.type === 'Resource') {
      g.act((a) => a.type === 'MaritimeTrade' && a.value.give === giveGood.value && a.value.get === want.value);
      const ri = RESOURCES.indexOf(giveGood.value);
      if (me().hand[ri] < me().tradeRatios[ri]) setBankGive(null);
    } else {
      g.act((a) => a.type === 'MaritimeTradeGood' && sameGood(a.value.give, giveGood) && sameGood(a.value.get, want));
      if (giveGood.type === 'Resource') {
        const ri = RESOURCES.indexOf(giveGood.value);
        if (me().hand[ri] < me().tradeRatios[ri]) setBankGive(null);
      } else {
        const ci = COMMODITIES.indexOf(giveGood.value);
        if (me().commodities[ci] < me().commodityTradeRatio) setBankGive(null);
      }
    }
  };

  return (
    <Modal title="交易" onClose={props.onClose} wide>
      <div class="tabs">
        <button classList={{ on: tab() === 'bank' }} onClick={() => setTab('bank')}>
          与银行
        </button>
        <Show when={!isPaired()}>
          <button classList={{ on: tab() === 'players' }} onClick={() => setTab('players')}>
            与玩家
          </button>
        </Show>
      </div>

      <Show when={tab() === 'bank'}>
        <p class="muted">选择要给出的资源{g.citadels ? '或商品' : ''}（自动使用最优港口/商馆比例），再选择目标。</p>
        <div class="bank-grid">
          <For each={RESOURCES}>
            {(r, i) => {
              const ratio = () => me().tradeRatios[i()];
              const can = () => me().hand[i()] >= ratio();
              const item: TradeGood = { type: 'Resource', value: r };
              return (
                <button
                  class="choice"
                  classList={{ on: bankGive() !== null && sameGood(bankGive()!, item) }}
                  disabled={!can()}
                  onClick={() => setBankGive(item)}
                >
                  <ResIcon r={i()} size="md" />
                  <span>
                    {ratio()} {RESOURCE_NAME[r]} → 1
                  </span>
                  <span class="muted">有 {me().hand[i()]}</span>
                </button>
              );
            }}
          </For>
          <Show when={g.citadels}>
            <For each={COMMODITIES}>
              {(c, i) => {
                const ratio = () => me().commodityTradeRatio;
                const can = () => me().commodities[i()] >= ratio();
                const item: TradeGood = { type: 'Commodity', value: c };
                return (
                  <button
                    class="choice"
                    classList={{ on: bankGive() !== null && sameGood(bankGive()!, item) }}
                    disabled={!can()}
                    onClick={() => setBankGive(item)}
                  >
                    <CommIcon c={i()} size="md" />
                    <span>
                      {ratio()} {COMMODITY_NAME[c]} → 1
                    </span>
                    <span class="muted">有 {me().commodities[i()]}</span>
                  </button>
                );
              }}
            </For>
          </Show>
        </div>
        <Show when={bankGive() !== null}>
          <p>换成：</p>
          <div class="choices row">
            <For each={RESOURCES}>
              {(r, i) => (
                <button
                  class="choice"
                  disabled={!canGetGood({ type: 'Resource', value: r })}
                  onClick={() => doBankTrade({ type: 'Resource', value: r })}
                >
                  <ResIcon r={i()} size="md" /> {RESOURCE_NAME[r]}
                </button>
              )}
            </For>
            <Show when={g.citadels}>
              <For each={COMMODITIES}>
                {(c, i) => (
                  <button
                    class="choice"
                    disabled={!canGetGood({ type: 'Commodity', value: c })}
                    onClick={() => doBankTrade({ type: 'Commodity', value: c })}
                  >
                    <CommIcon c={i()} size="md" /> {COMMODITY_NAME[c]}
                  </button>
                )}
              </For>
            </Show>
          </div>
        </Show>
      </Show>

      <Show when={tab() === 'players'}>
        <Show
          when={offer()}
          fallback={
            <>
              <div class="trade-cols">
                <div>
                  <h4>你给出</h4>
                  <For each={RESOURCES}>
                    {(_, i) => (
                      <Stepper
                        label={<ResIcon r={i()} size="md" />}
                        value={give()[i()]}
                        max={me().hand[i()]}
                        onChange={(v) => {
                          setGive(withCount(give(), i(), v));
                          if (v > 0) setGet(withCount(get(), i(), 0));
                        }}
                      />
                    )}
                  </For>
                </div>
                <div>
                  <h4>你想要</h4>
                  <For each={RESOURCES}>
                    {(_, i) => (
                      <Stepper
                        label={<ResIcon r={i()} size="md" />}
                        value={get()[i()]}
                        max={9}
                        onChange={(v) => {
                          setGet(withCount(get(), i(), v));
                          if (v > 0) setGive(withCount(give(), i(), 0));
                        }}
                      />
                    )}
                  </For>
                </div>
              </div>
              <footer>
                <span class="muted">双方都至少要给出一张牌</span>
                <button
                  class="primary"
                  disabled={setTotal(give()) === 0 || setTotal(get()) === 0}
                  onClick={() => g.proposeTrade(give(), get())}
                >
                  发起报价
                </button>
              </footer>
            </>
          }
        >
          {(o) => (
            <>
              <p class="offer">
                你给出 <SetView set={o().give} />，换取 <SetView set={o().get} />
              </p>
              <div class="choices">
                <For each={g.view().players.filter((p) => p.seat !== g.me)}>
                  {(p) => {
                    const resp = () => o().responses[p.seat];
                    return (
                      <div class="response">
                        <SeatDot seat={p.seat} /> {g.name(p.seat)}
                        <span class={`resp resp-${resp().toLowerCase()}`}>{RESPONSE_TEXT[resp()]}</span>
                        <Show when={g.legal().actions.some((a) => a.type === 'ConfirmTrade' && a.value.partner === p.seat)}>
                          <button class="primary" onClick={() => g.act((a) => a.type === 'ConfirmTrade' && a.value.partner === p.seat)}>
                            成交
                          </button>
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
              <footer>
                <button onClick={() => g.act((a) => a.type === 'CancelTrade')}>撤回报价</button>
              </footer>
            </>
          )}
        </Show>
      </Show>
    </Modal>
  );
}

export function GameOverModal(props: {
  game: Game;
  onAgain?: () => void;
  onLobby: () => void;
  onWatchReplay?: (record: ReplayRecord, link: string | null) => void;
  /** Online: the room, for the rating changes of a ranked game. */
  room?: RoomInfo | null;
}) {
  const g = props.game;
  // Your rating changed: show it in the lobby too.
  createEffect(() => {
    if (props.room?.rating.length) void refreshMyProfile();
  });
  const ranking = () =>
    g
      .view()
      .players.map((p) => ({ seat: p.seat, points: g.view().finalPoints?.[p.seat] ?? p.publicVictoryPoints }))
      .sort((a, b) => b.points - a.points);
  return (
    <Modal title={g.view().winner === g.me ? '你赢了！' : `${g.name(g.view().winner!)}获胜`}>
      <ol class="ranking">
        <For each={ranking()}>
          {(r) => (
            <li classList={{ winner: r.seat === g.view().winner }}>
              <SeatDot seat={r.seat} /> {g.name(r.seat)}
              <b>{r.points} 分</b>
            </li>
          )}
        </For>
      </ol>
      <p class="muted">共 {g.view().turn} 个回合</p>
      <Show when={props.room?.ranked && props.room}>{(room) => <RatingResult room={room()} />}</Show>
      <Show when={g.exportReplay}>
        {(load) => {
          const [busy, setBusy] = createSignal(false);
          const withRecord = async (use: (r: ReplayRecord) => void) => {
            if (busy()) return;
            setBusy(true);
            try {
              const record = await load()();
              if (record) use(record);
              else g.showToast('回放还没准备好，请稍后再试');
            } finally {
              setBusy(false);
            }
          };
          return (
            <div class="row replay-actions">
              <Show when={props.onWatchReplay}>
                {(watch) => (
                  <button disabled={busy()} onClick={() => void withRecord((r) => watch()(r, g.replayLink?.() ?? null))}>
                    观看回放
                  </button>
                )}
              </Show>
              <Show when={!EMBEDDED}>
                <button disabled={busy()} onClick={() => void withRecord(downloadReplay)}>
                  下载回放
                </button>
              </Show>
            </div>
          );
        }}
      </Show>
      <footer>
        <button onClick={props.onLobby}>返回大厅</button>
        <Show when={props.onAgain}>
          {(again) => (
            <button class="primary" onClick={() => again()()}>
              再来一局
            </button>
          )}
        </Show>
      </footer>
    </Modal>
  );
}

/** Citadels: three-track urban improvements and Metropolis crowning. */
export function CityUpgradeModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  const [metroTrack, setMetroTrack] = createSignal<CityTrack | null>(null);
  const progress = () => (g.me !== null ? g.view().players[g.me].cityProgress : [0, 0, 0]);
  const comm = () => g.view().me?.commodities ?? emptyComm();

  const upgradeOptions = (track: CityTrack) =>
    g.legal().actions.flatMap((a) => (a.type === 'UpgradeCity' && a.value.track === track ? [a.value.metropolis] : []));

  const clickUpgrade = (track: CityTrack) => {
    const opts = upgradeOptions(track);
    if (!opts.length) return;
    if (opts.length === 1) {
      g.act((a) => a.type === 'UpgradeCity' && a.value.track === track && a.value.metropolis === opts[0]);
    } else {
      setMetroTrack(track);
    }
  };

  return (
    <Modal title="城邦科技与大都会" onClose={props.onClose} wide>
      <p class="muted">
        拥有城市即可消耗对应商品升级；Lv3 解锁专属特权，率先达 Lv4 加冕大都会（+2 分且免疫黑帆攻城），他人先达 Lv5 可夺走。
      </p>
      <div class="tracks-grid">
        <For each={CITY_TRACKS}>
          {(track, idx) => {
            const lvl = () => progress()[idx()];
            const ci = () => COMMODITIES.indexOf(TRACK_COMMODITY[track]);
            const owner = () => g.view().citadels?.metropolisOwners[idx()] ?? null;
            const opts = () => upgradeOptions(track);
            return (
              <div class="track-card">
                <h4>
                  <span>{TRACK_NAME[track]}</span>
                  <span>Lv.{lvl()}/5</span>
                </h4>
                <div class="track-pips">
                  <For each={[1, 2, 3, 4, 5]}>{(k) => <span classList={{ on: lvl() >= k }} />}</For>
                </div>
                <p class="muted small">{TRACK_PERK[track]}</p>
                <p class="muted small">
                  大都会：{owner() !== null ? g.name(owner()!) : '尚未加冕'}
                </p>
                <Show
                  when={lvl() < 5}
                  fallback={<span class="muted small">已满级</span>}
                >
                  <button class="primary small" disabled={opts().length === 0} onClick={() => clickUpgrade(track)}>
                    升至 Lv.{lvl() + 1}（{lvl() + 1}×
                    <CommIcon c={ci()} />，现有 {comm()[ci()]}）
                  </button>
                </Show>
              </div>
            );
          }}
        </For>
      </div>
      <Show when={metroTrack()}>
        {(track) => (
          <div class="rules-box">
            <h4>选择一座普通城市加冕为【{TRACK_NAME[track()]}大都会】：</h4>
            <div class="choices row">
              <For each={upgradeOptions(track()).filter((v): v is number => v !== null)}>
                {(v, k) => (
                  <button
                    class="choice primary"
                    onClick={() => {
                      g.act((a) => a.type === 'UpgradeCity' && a.value.track === track() && a.value.metropolis === v);
                      setMetroTrack(null);
                    }}
                  >
                    城市 #{k() + 1}（格点 {v}）
                  </button>
                )}
              </For>
            </div>
          </div>
        )}
      </Show>
    </Modal>
  );
}

/** An offer from another player, shown to everyone else while it is open. */
export function IncomingTrade(props: { game: Game }) {
  const g = props.game;
  const offer = () => {
    const v = g.view();
    return g.me !== null && v.trade && v.current !== g.me ? v.trade : null;
  };
  const canAccept = () => g.legal().actions.some((a) => a.type === 'RespondTrade' && a.value.accept);
  return (
    <Show when={offer()}>
      {(o) => {
        const response = () => o().responses[g.me!];
        const from = () => g.view().current;
        return (
          <div class="panel incoming" role="dialog" aria-label="交易报价">
            <p>
              <SeatDot seat={from()} /> <b>{g.name(from())}</b> 想和你交易
            </p>
            <p class="offer">
              你得到 <SetView set={o().give} />，你给出 <SetView set={o().get} />
            </p>
            <Show
              when={response() === 'Pending'}
              fallback={
                <div class="row">
                  <span class="muted">{response() === 'Accepted' ? '你已接受，等待对方确认' : '你已拒绝'}</span>
                  <Show when={response() === 'Accepted'}>
                    <button onClick={() => g.act((a) => a.type === 'RespondTrade' && !a.value.accept)}>撤销</button>
                  </Show>
                </div>
              }
            >
              <div class="row">
                <button
                  class="primary"
                  disabled={!canAccept()}
                  title={canAccept() ? '' : '你没有足够的牌'}
                  onClick={() => g.act((a) => a.type === 'RespondTrade' && a.value.accept)}
                >
                  接受
                </button>
                <button onClick={() => g.act((a) => a.type === 'RespondTrade' && !a.value.accept)}>拒绝</button>
              </div>
            </Show>
          </div>
        );
      }}
    </Show>
  );
}
