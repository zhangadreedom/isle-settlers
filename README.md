# Isle Settlers

六边形岛屿上"开拓、交易、建造"的回合制桌游 Web 版：2–6 人联机对战、AI 对手、对局回放、类 3D 画风。名称与美术均为原创。

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

`E2E_PLAYERS=6` 改为 6 人局（扩展岛屿与特殊建造阶段）。脚本在终局后还会检查回放：下载回放文件、打开回放、拖动进度条、切换视角、播放，然后从大厅重新载入下载的文件。

在浏览器 URL 后加 `?renderer=webgl` 可以强制使用 WebGL2；加 `?debug` 会暴露测试钩子 `window.__isle`。

## 联机对战

服务器只有一个二进制，同时提供前端静态文件（默认 `web/dist`）和 `/api/ws` 游戏协议：

```bash
pnpm build
```

```bash
cargo run --release -p isle-server -- --addr 0.0.0.0:8080 --data-dir data
```

浏览器打开 `http://localhost:8080`，选"联机对战 → 创建房间"，把邀请链接发给朋友即可。每局结束后服务器会保存回放，结算界面可以观看、下载回放或复制分享链接（`#/replay/ID`，接口为 `GET /api/replays/{id}`）；本地对局的回放可以下载成文件，之后在大厅"打开回放文件"。服务器参数：

| 参数 | 默认值 | 说明 |
|---|---|---|
| `--addr` | `0.0.0.0:$PORT`（`PORT` 默认 8080） | 监听地址 |
| `--static-dir` | `web/dist`（存在时） | 前端构建产物 |
| `--data-dir` | `$ISLE_DATA_DIR`，否则不持久化 | 房间存档目录，重启后通过重放恢复对局；回放存在其中的 `replays/` 子目录（不设时只在内存里保留最近 256 局） |
| `--bot-delay` | `1.0` | AI 出手间隔的倍率，0 表示立即出手 |
| `--autopilot-secs` | `30` | 玩家掉线多久后由 AI 代打 |

也可以用 Docker 部署：

```bash
docker build -t isle-settlers .
```

```bash
docker run -p 8080:8080 -v isle-data:/data isle-settlers
```

联机端到端测试：两个相互隔离的浏览器分别扮演房主和客人，建房、加入、聊天、交易、刷新重连，一直打到终局。需要先启动服务器：

```bash
node web/e2e/online.cjs
```

## 当前进度

- [x] P0：规则引擎、基线 bot、自对弈基准、WASM 绑定、3D 棋盘骨架
- [x] P1：浏览器内人机对局（大厅、点击建造、交易、发展卡、弃牌与强盗、动画、观战模式）
- [x] P2：联机对战（WebSocket 服务器、房间与邀请链接、断线重连、掉线托管、聊天、持久化、Docker）
- [x] P3：困难 AI（确定化 + ISMCTS + 评估函数，锦标赛调参；浏览器端跑在 Web Worker 里）
- [ ] P4：5–6 人扩展 ✅、回放 ✅、账号与排位、扩展包
