---
description: 30 秒速判一只股票（lite 深度一把跑完 + 核心维度 + 评委投票 + 杀猪盘检测）
argument-hint: "[股票名称或代码]"
---

# 速判任务

用户输入: $ARGUMENTS

走 `deep-analysis` 的 **快速模式**（lite 深度，一把跑完，不做 agent role-play）：

```bash
uzi $ARGUMENTS --depth lite --no-browser
```

## 关注重点

**lite 档只跑 7 个核心维度**，其余 13 维不采集（白名单来自
`uzi_cli::profile::AnalysisProfile::fetchers_enabled`，过滤发生在 `uzi_data::collect`）：

| 维度 | 说明 |
|---|---|
| `0_basic` | 基础信息 |
| `1_financials` | 财报 |
| `2_kline` | K 线技术面 |
| `10_valuation` | 估值分位 |
| `11_governance` | 治理 / 减持 |
| `15_events` | 事件驱动 |
| `16_lhb` | 龙虎榜 |

未采集的维度在 `raw_data.json` 里以 `_pipeline.quality == "error"` 占位（仍存在，便于
下游遍历），自查与打分都会按 profile 把它们排除。

> ⚠️ **`18_trap` 不在 lite 白名单里**，`4_peers` / `5_chain` / `6_research` /
> `6_fund_holders` / `12_capital_flow` / `14_moat` / `17_sentiment` / `19_contests` 同样不采集。
> 需要杀猪盘结论请用 `--depth medium`（或 deep），或单独走 [`scan-trap`](scan-trap.md)。

评委层：lite 只跑 **10 位**（`investors_count: 10`），没有 agent role-play。

> 📌 **续跑时要带上同样的档位**。`--stage2` / `--stage-review` 若没显式给 `--depth`，
> 会自动沿用缓存里记录的档位（`_agent_review_context.json` 的 `depth`），所以
> `uzi <ticker> --depth lite --no-browser` 一条就能跑通；但**显式传了 `--depth medium`
> 去续跑一个 lite 快照，会因 13 维缺失被判 critical 而 BLOCKED**。

读缓存交叉核对：

```bash
jq '.vote_distribution' .cache/{ticker}/panel.json
jq '.dimensions["18_trap"].data' .cache/{ticker}/raw_data.json   # 需 medium/deep 才有
```

速判只摘 Top 5 看多 / Top 5 看空（lite 下池子只有 10 人，通常不足 5 条，按实际条数读）：

```bash
jq '[.investors[] | select(.signal=="bullish")] | sort_by(-.score) | .[0:5] | .[] | {name, score, comment}' .cache/{ticker}/panel.json
jq '[.investors[] | select(.signal=="bearish")] | sort_by(.score) | .[0:5] | .[] | {name, score, comment}' .cache/{ticker}/panel.json
```

## 输出

输出**精简版 markdown**（不出 HTML 报告），包含：

- 一句话定调
- 综合评分
- 66 位大佬投票分布（重点列 Top 5 看多 / Top 5 看空）
- 杀猪盘安全等级（🟢/🟡/🟠/🔴）
- 关键风险 1-2 条

目标：1-2 分钟内给出回答。
