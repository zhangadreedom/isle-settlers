# Isle Settlers

六边形岛屿上"开拓、交易、建造"的回合制桌游 Web 版：联机对战、AI 对手、类 3D 画风。名称与美术均为原创。

- 技术栈与架构决策：[`docs/adr/0001-tech-stack.md`](docs/adr/0001-tech-stack.md)
- 规则实现说明：[`docs/rules.md`](docs/rules.md)

## 仓库结构

```
crates/
  isle-core/   规则引擎（Rust，零依赖）：编译期拓扑表、Copy 状态、规则、合法走法
  isle-ai/     AI 对手：Bot trait、随机与加权随机基线
  isle-sim/    自对弈与引擎基准 CLI
  isle-wasm/   WebAssembly 绑定
web/           Vite + SolidJS + Three.js 前端
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

启动开发服务器（先构建 WASM）：

```bash
pnpm dev
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

带每步不变量检查的自对弈：

```bash
cargo run --release -p isle-sim -- --games 5000 --check
```

在浏览器 URL 后加 `?renderer=webgl` 可以强制使用 WebGL2。

## 当前进度

- [x] P0：规则引擎、基线 bot、自对弈基准、WASM 绑定、3D 棋盘骨架
- [ ] P1：浏览器内人机对局（点击建造、交易面板、动画）
- [ ] P2：联机服务器（WebSocket、房间、邀请链接、断线重连）
- [ ] P3：AI 分层（价值函数 → Expectimax → ISMCTS）
- [ ] P4：5–6 人扩展、扩展包、账号与排位
