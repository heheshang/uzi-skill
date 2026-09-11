# UZI-Skill · Claude Code Context

> 本文件供 Claude Code 自动读取，提供项目上下文。

## 这是什么

一个股票深度分析 plugin（上游 Python 版的 **Rust 重实现**，单一二进制 `uzi`）。
用户说"分析 XXX"时，你应该自动触发 `deep-analysis` skill。

## 核心技能

| Skill | 触发条件 | 说明 |
|---|---|---|
| `deep-analysis` | 用户提到"分析/研究/估值/DCF/值不值得买"等 | 22 维数据 + 66 评委（9 大流派）+ Bloomberg 风格报告 |
| `investor-panel` | 用户要求"只看评委/大佬怎么看" | 单独跑投资者面板（读 `panel.json`） |
| `lhb-analyzer` | 用户提到"龙虎榜/游资/营业部" | 龙虎榜专项分析 |
| `trap-detector` | 用户提到"杀猪盘/有没有问题/安全吗" | 杀猪盘检测 |

## 工作流 · 深浅两档

**快速路径（默认）**：用户说"分析/看看"、或 lite/medium 档时，走 CLI 直跑。

```bash
uzi <ticker> --depth lite --no-browser     # 1-2 分钟 · 7 维核心 + 10 评委
uzi <ticker> --depth medium --no-browser   # 5-8 分钟，默认完整度
```

lite/medium 档 `agent_analysis.json` 缺失自动降级 warning，照样出 HTML。**不需要 role-play 66 评委**。

**深度路径（deep 档必须走）**：当用户要 `--depth deep`、DCF / IC memo / 首次覆盖 / 投委会备忘录等
深度产物时，**你必须介入 role-play，不能只跑 CLI**：

1. `uzi <ticker> --stage1` — 脚本采集 22 维数据 + 机构建模 + 规则引擎骨架分
2. **你介入（必走）** — 读 `panel.json` + `skills/deep-analysis/personas/*.yaml`，以 66 评委身份
   逐个分析当前股票，写 `agent_analysis.json`（含 `analysis_input_hash`；可选 `per_investor_override`）
3. `uzi <ticker> --stage2` — 自动合并你的 role-play 成果，生成报告

> ⚠️ **deep 档不要直接 `uzi <ticker> --depth deep` 一把梭**——那是纯规则输出。deep 的意义就在于
> 你代入角色做判断。见 `AGENTS.md` 路径判断表。

详细流程见 `AGENTS.md` / `skills/deep-analysis/SKILL.md`。

> ⛔ **不要读源码**（`HARD-GATE-NO-SOURCE-READ`）：运行时不需要 Rust 源码 —— 不要为理解校验规则 /
> 字段语义去读 `crates/**/*.rs`，也不要从 GitHub 拉源码；契约读 skill 文档，自查结论用
> `uzi <ticker> --stage-review`。

## 重要文件

- `AGENTS.md` — 完整 agent 指令
- `skills/deep-analysis/SKILL.md` — 深度分析工作流
- `crates/uzi-cli/src/stages.rs` — 主引擎（`stage1` / `stage2` 编排；上游 rrt 引擎的落点 —— **仅源码维护者需要，运行时读的是二进制**）
- `commands/analyze-stock.md` — `/analyze-stock` 命令
