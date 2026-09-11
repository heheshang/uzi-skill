# UZI-Skill · Gemini CLI 指令

## 安装

```bash
gemini extensions install https://github.com/heheshang/uzi-skill
```

更新：

```bash
gemini extensions update stock-deep-analyzer
```

## 使用

对 Gemini 说"分析 贵州茅台"，或直接执行：

```bash
./uzi 贵州茅台 --no-browser
```

> 仓库根目录自带预编译 `./uzi`（macOS arm64），无需构建；改代码 / 其他平台才需
> `cargo build --release -p uzi-cli`（产物 `target/release/uzi`）。

> 本项目零外部依赖（Rust 单一二进制 `uzi`）：不需要 `pip install`，也没有 `requirements.txt`。

## 完整流程

参考 `AGENTS.md` 和 `skills/deep-analysis/SKILL.md`。

核心是两段式：
1. `uzi <ticker> --stage1` — 数据采集 + 机构建模 + 22 维评分 + 规则引擎评委骨架分，然后停下
2. Agent 分析 — 读 `panel.json`，逐组 role-play 66 评委，写 `agent_analysis.json`（含 `analysis_input_hash`）
3. `uzi <ticker> --stage2` — 合并你的判断，生成 Bloomberg 风格 HTML 报告
