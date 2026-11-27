# Isle Settlers

六边形岛屿上"开拓、交易、建造"的回合制桌游 Web 版：2–6 人联机对战、AI 对手、对局回放、航海扩展（造船出海、海盗、金矿、登岛奖励）、类 3D 画风。名称、地图与美术均为原创。

- 技术栈与架构决策：[`docs/adr/0001-tech-stack.md`](docs/adr/0001-tech-stack.md)
- 规则实现说明：[`docs/rules.md`](docs/rules.md)

## 仓库结构

```
crates/
  isle-core/   规则引擎（Rust，零依赖）：编译期拓扑表、Copy 状态、规则、合法走法
  isle-proto/  按玩家投影的视图（PlayerView）与线上类型；生成前端 TS 类型
  isle-ai/     AI 对手：ISMCTS（困难）、GreedyBot（普通）、随机基线；确定化与评估函数
  isle-sim/    自对弈与引擎基准 CLI
  isle-wasm/   WebAssembly 绑定：浏览器内对局托管（MatchHost / LocalMatch）
  isle-server/ 联机服务器（axum + WebSocket）：房间、邀请链接、断线重连、托管、持久化
web/           Vite + SolidJS + Three.js 前端（src/generated/proto.ts 为生成文件）
docs/          架构决策与规则说明
scripts/       构建脚本
```

## 环境要求

- Rust stable（`rust-toolchain.toml` 会自动安装 `wasm32-unknown-unknown` 目标）
- Node 22+ 与 pnpm 10
- 与 `crates/isle-wasm` 中版本一致的 wasm-bindgen CLI：

```bash
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

## 常用命令

安装前端依赖：

```bash
pnpm install
```

启动前端开发服务器（先构建 WASM）。`/api` 会被代理到 `localhost:8080`：

```bash
pnpm dev
```

另开一个终端启动联机服务器，单机模式不需要它：

```bash
cargo run -p isle-server
```

生产构建，输出到 `web/dist`：

```bash
pnpm build
```

运行全部 Rust 测试：

```bash
cargo test --workspace
```

引擎基准（加权随机 bot 自对弈）：

```bash
cargo run --release -p isle-sim -- --games 40000
```

带每步不变量检查的自对弈（`--players 2-6`，5–6 人使用扩展岛屿）：

```bash
cargo run --release -p isle-sim -- --games 5000 --check
```

```bash
cargo run --release -p isle-sim -- --games 3000 --players 6 --check
```

航海扩展（群岛或迷雾地图，2–4 人）加 `--scenario archipelago` 或 `--scenario fog`，结果里会多出人均登岛奖励、终局船数与翻开的迷雾格数：

```bash
cargo run --release -p isle-sim -- --games 1000 --scenario archipelago --players 4 --bot greedy --check
cargo run --release -p isle-sim -- --games 1000 --scenario fog --players 4 --bot greedy --check
```

确定性指纹：对一批固定种子的对局取哈希，应输出 `b862d3b3842bae2d`。修改引擎时用它确认基础局的行为没有变（已存档的房间和回放依赖这一点）：

```bash
cargo run --release -p isle-ai --example fingerprint
```

AI 强度对比：`--rotate` 每局轮换座位，按 bot 统计胜率并给出 95% 置信区间。`mcts:N/key=value/...` 可以调整搜索参数，比如 `mcts:1000/r=0/c=0.3`，键名见 `MctsConfig::set`：

```bash
cargo run --release -p isle-sim -- --games 400 --rotate --bots mcts,greedy,greedy,greedy
```

修改 Rust 线上类型后重新生成前端 TS 类型（不同步时测试会失败）：

```bash
ISLE_UPDATE_TS=1 cargo test -p isle-proto --features ts --test typescript
```

端到端测试：通过真实界面以人类身份完整打一局（需要 Playwright + Chromium，并先启动 `pnpm preview`）：

```bash
node web/e2e/play-through.cjs
```

`E2E_PLAYERS=6` 改为 6 人局（扩展岛屿与特殊建造阶段）。`E2E_SCENARIO=archipelago` 改玩航海群岛：开局优先选海边，在海岸边选择修路还是造船，造船、移船、把海盗移到海上，选金矿资源，并检查至少造过一艘船（联机测试 `online.cjs` 同样支持这个变量，并检查客人在等待房间看到了航海标签）。脚本在终局后还会检查回放：下载回放文件、打开回放、拖动进度条、切换视角、播放，然后从大厅重新载入下载的文件，并从大厅的“本机回放”列表打开这一局。

已结束的本机对局会自动保存在浏览器，最多保留最近 20 局。刷新后可在大厅展开「本机回放」，按人数、赢家、地图种子或日期查找。记录不上传，也不会跨浏览器或站点同步；禁用浏览器存储时仍可在结算页直接观看回放。

测试嵌入版时，把 `E2E_URL` 指向静态托管的 `web/dist-embed`（可放在子路径下），并设置 `E2E_EMBED=1`。脚本会检查联机和回放下载入口未显示，仍会从开局打到结算并验证内置回放。可加 `E2E_AI=困难` 测试困难 AI。

也可以用自带静态服务器的嵌入版专用测试：它把 `web/dist-embed` 挂在子路径 `/sub/` 下，禁用 localStorage 模拟沙箱，用困难 AI 打完一局 2 人局并检查回放；出现页面错误、触发下载、请求越出子路径或文件缺失、出现联机或下载入口、渲染器不是 WebGL2、没有加载 Worker 或 WASM 时判失败（先运行 `pnpm build:embed`，`E2E_AI`、`E2E_PLAYERS` 同上）：

```bash
node web/e2e/embed.cjs
```

在浏览器 URL 后加 `?renderer=webgl` 可以强制使用 WebGL2；加 `?debug` 会暴露测试钩子 `window.__isle`。

只要单机版、不需要服务器时，可以构建一个可放在静态托管或沙箱网页里的版本（资源用相对路径，隐藏联机和下载按钮，固定使用 WebGL2），输出到 `web/dist-embed`。从仓库根目录运行；与 `pnpm build` 一样，Windows（PowerShell/CMD）、Linux 和 macOS 使用同一个命令，不需要 Bash，且会先构建 WASM：

```bash
pnpm build:embed
```

## 联机对战

服务器只有一个二进制，同时提供前端静态文件（默认 `web/dist`）和 `/api/ws` 游戏协议：

```bash
pnpm build
```

```bash
cargo run --release -p isle-server -- --addr 0.0.0.0:8080 --data-dir data
```

浏览器打开 `http://localhost:8080`，选"联机对战 → 创建房间"，把邀请链接发给朋友即可。每局结束后服务器会保存回放，结算界面可以观看、下载回放或复制分享链接（`#/replay/ID`，接口为 `GET /api/replays/{id}`）；本地对局的回放可以下载成文件，之后在大厅"打开回放文件"，或直接从大厅"本机回放"列表打开。服务器参数：

| 参数 | 默认值 | 说明 |
|---|---|---|
| `--addr` | `0.0.0.0:$PORT`（`PORT` 默认 8080） | 监听地址 |
| `--static-dir` | `web/dist`（存在时） | 前端构建产物 |
| `--data-dir` | `$ISLE_DATA_DIR`，否则不持久化 | 房间存档目录，重启后通过重放恢复对局；回放存在其中的 `replays/` 子目录（不设时只在内存里保留最近 256 局） |
| `--bot-delay` | `1.0` | AI 出手间隔的倍率，0 表示立即出手 |
| `--autopilot-secs` | `30` | 玩家掉线多久后由 AI 代打 |
| `--no-accounts` | 有 `--data-dir` 时开启账号 | 关闭账号功能 |
| `--session-days` | `30` | 登录有效天数（1–366） |
| `--placement-games` | `5` | 定级赛局数：打满之前没有段位，也不上排行榜（0–100） |
| `--client-ip-header` | 无 | 放在反向代理后面时，从这个请求头取客户端地址用于登录限流（如 `X-Forwarded-For`，取最后一个地址）。只在代理会覆盖或追加这个头时设置，否则客户端可以伪造 |

### 账号

服务器带 `--data-dir` 时提供可选账号（存在 `accounts.sqlite3` 里，密码用 Argon2id 哈希）。不登录也能玩；登录后：

- 用户名就是你在房间里的名字，座位旁有 ✓ 标记，别人改昵称冒充不了；
- 座位绑定账号，在另一台设备（或清空浏览器后）登录并打开邀请链接，就能回到自己的座位。

用户名为字母、数字、`_`、`-`，不区分大小写，最长 32 位；密码至少 12 个字符。注册和登录每个地址先给 10 次，之后每 6 秒恢复一次（IPv6 按 /64 计）。接口（请求体和返回值的类型见 `crates/isle-proto/src/account.rs`，前端类型在 `web/src/generated/proto.ts`）：

| 请求 | 说明 |
|---|---|
| `POST /api/accounts` `{username, password}` | 注册并登录，返回 `{account, token, expiresAt}` |
| `POST /api/sessions` `{username, password}` | 登录，返回同上 |
| `GET /api/session` | 当前登录信息，需要 `Authorization: Bearer <token>` |
| `DELETE /api/session` | 退出登录 |
| `DELETE /api/sessions` | 退出所有设备 |

失败时返回 `{error}`，错误码列在 `ApiError` 的注释里。`GET /api/health` 的 `accounts` 字段表示服务器是否开启了账号。

### 排位

创建房间时选「排位」或「休闲」（开局前房主可以改）。排位房间里至少有两名登录玩家时，终局后为这些玩家计算排位分，AI 和游客不计；名次按“赢家第一，其余按分数，同分并列”。结算界面显示每人的分数变化，大厅的「排行榜」页有自己的排位、最近的排位对局（可以直接打开回放）和排行榜。

- 算法是 Weng-Lin 贝叶斯评分（Plackett-Luce 模型，OpenSkill 用的就是它），专为多人自由混战设计，支持并列名次。
- 显示的排位分是保守估计：`1000 + 25 × (μ − 3σ)`。所有人从 1000 起步，打得越多、系统越有把握，分数越可信，运气好的新人也不会一两局就冲到榜首。
- 前 5 局是定级赛，期间分数照常变化，但没有段位，也不上排行榜。
- 段位：青铜（<1100）、白银（1100）、黄金（1250）、铂金（1400）、钻石（1550）、大师（1700+）。

| 请求 | 说明 |
|---|---|
| `GET /api/leaderboard?limit=50&offset=0` | 已定级玩家，按排位分从高到低 |
| `GET /api/players/{username}` | 玩家的排位分、段位、名次和最近 20 局排位对局 |

排位数据和账号存在同一个数据库里；旧版本的数据库在启动时自动升级。

也可以用 Docker 部署：

```bash
docker build -t isle-settlers .
```

```bash
docker run -p 8080:8080 -v isle-data:/data isle-settlers
```

联机端到端测试：两个相互隔离的浏览器分别扮演房主和客人，建房、加入、聊天、交易、刷新重连，一直打到终局。服务器开了账号时（带 `--data-dir`），房主还会注册、退出、输错密码、重新登录、刷新后保持登录，并用第三个浏览器登录同一账号、从邀请链接回到房主的座位；客人在邀请页注册，两人打一局排位赛，检查房主切换排位/休闲、双方结算时的分数变化，以及大厅排行榜页里的这局对局和它的回放。需要先启动服务器：

```bash
node web/e2e/online.cjs
```

## 当前进度

- [x] P0：规则引擎、基线 bot、自对弈基准、WASM 绑定、3D 棋盘骨架
- [x] P1：浏览器内人机对局（大厅、点击建造、交易、发展卡、弃牌与强盗、动画、观战模式、嵌入单人版 `build:embed`）
- [x] P2：联机对战（WebSocket 服务器、房间与邀请链接、断线重连、掉线托管、聊天、持久化、Docker）
- [x] P3：困难 AI（确定化 + ISMCTS + 评估函数，锦标赛调参；浏览器端跑在 Web Worker 里）
- [ ] P4：5–6 人扩展 ✅、回放与本机回放列表 ✅、账号（注册登录接口、前端登录、座位绑定账号、登录限流）✅、排位（评分、段位、排行榜、玩家主页）✅、航海扩展（群岛与迷雾双剧本、船与移船、海盗、金矿、迷雾翻牌奖励、登岛奖励，AI 会出海探索）✅、其他扩展包
