# UZI-Skill · Codex 安装指南

## 自动安装（推荐）

在 Codex 环境中执行：

```bash
git clone https://github.com/heheshang/uzi-skill.git && cd UZI-Skill
cargo build --release -p uzi-cli
./target/release/uzi 600519.SH --no-browser
```

安装完成。直接对 Codex 说"分析 贵州茅台"即可。

## 工作原理

Codex 会自动读取仓库根目录的 `AGENTS.md`，了解可用命令：

| 你说的话 | Codex 执行的命令 |
|---|---|
| 分析 贵州茅台 | `uzi 贵州茅台 --no-browser` |
| 分析 AAPL | `uzi AAPL --no-browser` |
| 远程分析 002273 | `uzi 002273 --remote` |

## 两段式深度分析（推荐）

Codex 作为 agent 应该分两步执行，中间自己做分析：

```bash
# Stage 1: 数据采集 + 规则引擎骨架分
uzi 600519.SH --stage1

# 此时 Codex 应该：
# 1. 读 .cache/600519.SH/panel.json 中 66 位评委骨架分
# 2. 对每组投资者做 role-play 分析
# 3. 把定性结论写入 .cache/600519.SH/agent_analysis.json

# Stage 2: 生成报告
uzi 600519.SH --stage2
```

## 快速模式

不需要 agent 介入，一把跑完：

```bash
uzi 600519.SH --no-browser
```

## 远程查看

不在电脑前时：

```bash
uzi 600519.SH --remote
```

会生成 `https://xxx.trycloudflare.com` 公网链接。

## 依赖

- Rust 工具链（`cargo build --release -p uzi-cli` 产出 `target/release/uzi`）
- 零 API key、零外部依赖（无任何运行时包安装步骤）
- 报告渲染首选系统已装的 Chromium 系浏览器（Chrome/Chromium/Edge/Brave），可用 `uzi --browser-check` 确认

## 目录结构

```
UZI-Skill/
├── crates/uzi-cli/           ← 单一二进制入口 uzi
├── AGENTS.md                 ← Codex 自动读取
├── skills/deep-analysis/     ← 核心分析工作流
│   ├── SKILL.md              ← 分析师手册
│   └── personas/             ← 66 位评委 persona 定义
└── commands/                 ← 命令卡
```

## 常见问题

**Q: 跑完看不到报告？**
A: 报告在 `reports/{ticker}_{YYYYMMDD}/full-report-standalone.html`，用 `--remote` 可以生成公网链接。

**Q: 依赖安装失败？**
A: 确保 Rust 工具链可用（`cargo --version`）。本项目零外部依赖，`uzi` 不会在运行时联网安装任何东西。

**Q: 中文名识别不了？**
A: 直接用代码：`600519.SH`（上海）、`002273.SZ`（深圳）、`00700.HK`（港股）、`AAPL`（美股）。
