---
name: trap-detector
description: 杀猪盘检测器。当用户提到"朋友推荐"、"群里说"、"老师带"、"内幕消息"、"小红书 / 抖音看到推荐"等关键词，或显式要求"看看是不是杀猪盘 / 检测一下风险 / 这只票安全吗"时使用。扫描 8 个信号给出风险评级 🟢🟡🟠🔴。
version: 3.9.4
author: FloatFu-true
license: MIT
metadata:
  tags: [finance, a-share, trap-detection, risk, pump-and-dump, fraud-detection]
  related_skills: [deep-analysis]
---

# Trap Detector · 杀猪盘检测

## 这个 skill 在本仓库的形态

杀猪盘检测是**维度 `18_trap`**，随 Stage 1 一起产出；真实 web search 扫描 8 个信号。

| 上游 | 本仓库实现 |
|---|---|
| `fetch_trap_signals.py` | `crates/uzi-data/src/fetch/trap_signals.rs` |
| `references/eight-signals.md` | 信号定义内联在本模块的 `SIGNALS` 常量 |

```bash
uzi <ticker> --stage1
jq '.dimensions["18_trap"].data' .cache/<ticker>/raw_data.json
```

> 扫描走 `web_search`（ddgs），受 `UZI_DDG_BUDGET` / `UZI_DDG_TIMEOUT` 约束。
> 检索受限时信号可能命中不全 —— 此时**必须**说明"检索受限"，不能当作"安全"。

## 📚 八信号详解

[`references/eight-signals.md`](references/eight-signals.md) —— 8 个信号的真实检索 query、
命中关键词、判定规则与风险评级映射，已按 `uzi_data::fetch::trap_signals` 校正。

## 触发场景

- 用户输入含关键词：朋友推荐、群里、老师带、内幕、必涨、翻倍、暴涨、稳赚、跟单
- 显式要求：检测一下、是不是杀猪盘、安不安全、被套路了吗

## 8 信号扫描清单

| # | 信号 | 检索 query（`{name}` = 股票名） |
|---|---|---|
| 1 | 大量低质量账号同时推荐 | `{name} 强烈推荐 必涨`、`{name} 内部消息 暴涨` |
| 2 | 推荐话术模板化 | `{name} 主力建仓完毕 即将爆发`、`{name} 翻倍 目标价` |
| 3 | 付费社群 / VIP 直播间引流 | `{name} 股票 微信群`、`{name} 老师 带单 VIP 直播间` |
| 4 | 基本面与热度脱节 | `{name} 业绩亏损 推荐 暴涨`、`{name} ST 推荐 拉升` |
| 5 | K 线异常配合 | `{name} 异动 操纵 拉升` |
| 6 | 老师 / 股神人设推广 | `{name} 老师 股神 跟单`、`{name} 实盘 老师` |
| 7 | 跨平台联动推广 | `小红书 {name} 股票 推荐`、`抖音 {name} 股票` |
| 8 | 虚假研报 / 伪造消息 | `{name} 虚假研报 谣言`、`{name} 辟谣 澄清` |

> **命中判定**：每个信号有若干 `positive_kws`；**同一信号命中 ≥ 2 个关键词**才算该信号
> 命中（`hits.len() >= 2`）。命中 3 个以上时该条 `severity` 标为 `high`，否则 `medium`。
> 所以"信号数"是一个偏保守的计数 —— 单关键词命中不计入。

## 风险评级

| 命中信号数 | 评级 | `trap_score` | 建议 |
|---|---|---|---|
| 0–1 | 🟢 安全 | 9 | 数据正常，未发现明显推广痕迹 |
| 2–3 | 🟡 注意 | 7 | 发现 N 个推广信号，建议核实信息源 |
| 4–5 | 🟠 警惕 | 4 | 发现 N 个推广信号，强烈建议谨慎 |
| 6+ | 🔴 高度可疑 | 1 | 发现 N 个推广信号，强烈建议回避，疑似杀猪盘特征 |

> `trap_score` **是反向分**：越高越安全（1=最危险，9=最安全）。

## 输出 · `18_trap.data`

```json
{
  "trap_level": "🟢 安全",
  "trap_score": 9,
  "signals_hit": "0/8",
  "signals_hit_count": 0,
  "signals_hit_detail": [
    {
      "id": 3,
      "name": "付费社群/VIP直播间引流",
      "evidence_kws": ["微信群", "老师带"],
      "severity": "high"
    }
  ],
  "recommendation": "数据正常，未发现明显推广痕迹。",
  "evidence_count": 0,
  "high_risk_kw": "未发现",
  "snippets": ["..."]
}
```

`source` 为 `web_search:ddgs + 8-signal keyword scan`。
`signals_hit_detail[].evidence_kws` 是命中的关键词（最多 3 个）；
`signals_hit_detail[].snippets` 不存在 —— 原始检索片段统一在顶层 `snippets`。

## 用户关键词加权

用户话术本身是证据，出现下列词时**信号严重程度自动升级**：

| 用户原话 | 加权 |
|---|---|
| "朋友推荐我" / "群里有人说" / "老师带我" | +1 |
| "内幕消息" / "稳赚不赔" | +2 |
| "必涨" / "翻倍" / "暴涨" | +1 |

加权后要落到 `recommendation` 的措辞上，不能只记在报告角落。

## 完成检查

- [ ] 8 个信号**每个**都给出"命中 / 未命中 / 数据不足"，不能省略
- [ ] 非 🟢 时必须给**至少 1 条具体证据 URL**
- [ ] `recommendation` 必须有内容
- [ ] ≥ 4 信号时，`recommendation` 必须以"强烈建议谨慎"或"强烈建议回避"开头
- [ ] 检索受限时明确说明，不得把"搜不到"等同于"安全"
- [ ] `18_trap` 已计入 22 维评分（权重 5）—— 不要与评分层的结论互相矛盾

## 免责

本检测基于公开检索，**不构成投资建议**。给用户的结论必须同时给出证据与不确定性。
