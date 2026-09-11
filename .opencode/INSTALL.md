# UZI-Skill · OpenCode 安装指南

## 安装

```bash
git clone https://github.com/heheshang/uzi-skill.git && cd UZI-Skill
cargo build --release -p uzi-cli
```

## 使用

对 OpenCode 说：

> 分析 贵州茅台

或直接执行：

```bash
./target/release/uzi 600519.SH --no-browser
```

## 两段式深度分析

```bash
# Stage 1: 数据采集 + 骨架分
uzi 600519.SH --stage1

# Agent 分析（读 .cache/600519.SH/panel.json，逐组分析 66 位评委，
# 结论写入 .cache/600519.SH/agent_analysis.json）

# Stage 2: 生成报告
uzi 600519.SH --stage2
```

## 远程查看

```bash
uzi 600519.SH --remote
```

## 更多信息

- `AGENTS.md` — Agent 指令
- `skills/deep-analysis/SKILL.md` — 完整分析师手册
- `README.md` — 项目介绍
