---
name: investor-panel
description: 66 位投资大佬评审团。给定一只股票的 dimensions 与 raw_data，让 66 位投资者各自按自己的方法论打分并输出 Signal（signal/confidence/score/verdict/comment）。覆盖经典价值派、成长投资派、宏观对冲派、技术趋势派、中国价投派、A股游资派、量化系统派、科技领袖派、AI 卡位猎手 9 大流派。当用户请求"评审团/大佬怎么看/某某会买吗/做一次大佬投票"时使用。
version: 3.9.4
author: FloatFu-true
license: MIT
metadata:
  tags: [finance, investor-panel, voting, role-play, a-share, value-investing, growth-investing]
  related_skills: [deep-analysis]
---

# Investor Panel · 评审团

## 📚 流派方法论（`references/`）

| 文件 | 内容 |
|---|---|
| [`references/group-a-classic-value.md`](references/group-a-classic-value.md) | A 经典价值（6） |
| [`references/group-b-growth.md`](references/group-b-growth.md) | B 成长投资（9） |
| [`references/group-c-macro-hedge.md`](references/group-c-macro-hedge.md) | C 宏观对冲（7） |
| [`references/group-d-technical.md`](references/group-d-technical.md) | D 技术趋势（4） |
| [`references/group-e-china-value.md`](references/group-e-china-value.md) | E 中国价投（7） |
| [`references/group-f-china-youzi.md`](references/group-f-china-youzi.md) | F A股游资（24）+ 射程规则 |
| [`references/group-g-quant.md`](references/group-g-quant.md) | G 量化系统（4） |
| [`references/group-i-serenity.md`](references/group-i-serenity.md) | I AI 卡位猎手 Serenity（1） |
| [`references/serenity-voice.md`](references/serenity-voice.md) | Serenity 语言风格与 signature lines |
| [`references/quotes-knowledge-base.md`](references/quotes-knowledge-base.md) | **语料库**：46 位人物 + 7 位席位类游资的真实公开语录与风格 —— 写 `comment` 前必读 |

> H 科技领袖（4）目前**没有独立方法论文件**（上游亦无）；按各自 persona 风格演绎。

## 这个 skill 在本仓库的形态

评委不单独成命令，而是**流水线 Task 3** 的产物：跑一次 Stage 1 就会写出
`.cache/{ticker}/panel.json`。

| 上游 | 本仓库实现 |
|---|---|
| `lib/investor_db.py` | `crates/uzi-investors/src/db.rs` + `src/data/investors.json`（66 位，内嵌） |
| `lib/investor_criteria.py` | `crates/uzi-investors/src/criteria.rs` |
| `lib/investor_evaluator.py` | `crates/uzi-investors/src/evaluator.rs` |
| `lib/investor_knowledge.py` | `crates/uzi-investors/src/knowledge.rs` + `src/data/knowledge.json` |
| `lib/investor_personas.py` | `crates/uzi-investors/src/personas.rs` + `src/data/persona_pools.json` |
| `lib/personas.py`（YAML） | `crates/uzi-investors/src/persona_yaml.rs` |
| `lib/seat_db.py` | `crates/uzi-investors/src/seat_db.rs` |
| 面板汇总/共识 | `crates/uzi-pipeline/src/panel.rs` |

```bash
uzi <ticker> --stage1      # 产出 panel.json（Task 3）
```

> 只要投票、不想要整份报告时：仍走 `--stage1`，然后直接读 `panel.json`，不必跑 `--stage2`。

## 输入

- `.cache/{ticker}/dimensions.json` — 22 维评分
- `.cache/{ticker}/raw_data.json` — 原始数据
- `investors.json` — 66 位元数据（`id` / `name` / `group` / `fields` 白名单 / `mandate`）
- `seats.json` — 23 位游资席位与射程规则

## 输出 · `panel.json`

顶层字段：

| 字段 | 含义 |
|---|---|
| `panel_consensus` | 多头共识分（0–100），见下方公式 |
| `consensus_valid` | `hollow_pct < 20` 才为 `true` |
| `hollow_verdicts` / `hollow_pct` / `hollow_ids` | 空洞裁决（无实质依据的看多）数量与占比 |
| `consensus_warning` | 非空时表示共识不可信及原因 |
| `signal_distribution` | `{bullish, neutral, bearish, skip}` |
| `vote_distribution` | `{strongly_buy, buy, watch, wait, avoid, n_a, skip}` |
| `school_scores` | 每个流派的组内共识、均分、活跃人数 |
| `long_active` | 参与多头共识的活跃人数 |
| `short_consensus` | 空头专册：`{total, active, skip, short_candidates, no_short_thesis, avg_score, top_short_candidates}` |
| `consensus_formula` | 公式版本与全部中间量（可审计） |
| `investors` | 66 份 Signal |

单份 Signal：

```json
{
  "investor_id": "buffett",
  "name": "沃伦·巴菲特",
  "group": "A",
  "mandate": "long",
  "avatar": "avatars/buffett.svg",
  "signal": "neutral",
  "confidence": 30,
  "score": 27,
  "verdict": "不适合",
  "headline": "一句话结论",
  "comment": "用该投资者语言风格的金句 1-2 句",
  "reasoning": "1-3 句具体逻辑",
  "pass": ["..."],
  "fail": ["..."],
  "weight_pass": 0,
  "weight_total": 0,
  "ideal_price": 16.89,
  "period": "3-5 年",
  "time_horizon": "3-5 年",
  "position_sizing": "标准仓",
  "what_would_change_my_mind": "什么情况下会改主意"
}
```

`signal` ∈ `bullish` / `bearish` / `neutral` / **`skip`**。
`verdict` ∈ `强烈买入` / `买入` / `关注` / `观望` / `等待` / `回避` / `不达标` / `不适合`。

### ⚠️ 分布**不等于** 66

`signal_distribution` / `vote_distribution` 只统计**多头簿**，**不含 `mandate == "short"`
的评委**（他们进 `short_consensus`，并被记入 `consensus_formula.short_excluded`）。

实测 600519.SH：66 位评委里 23 位 `skip`、2 位空头被排除 →
`signal_distribution` 四桶之和 = **64**，而非 66。
所以**不要**把"四桶之和 = 评委人数"当作校验条件。

### 共识公式（v2.15.5）

```
consensus_raw   = 0.65 * score_mean + 0.35 * vote_weighted
panel_consensus = polarize(consensus_raw, k = 1.3)
```

- `neutral_weight = 0.6` —— 中性票半权计入投票部分（修正旧公式里中性 0 权重的问题）
- `polarize` 把中间值推向两端，放大分歧
- `consensus_formula` 字段回写 `score_mean` / `vote_weighted` / `consensus_raw` / `consensus_final`
  与 `bullish` / `neutral_weighted` / `bearish` / `skip` / `active` / `short_excluded`，
  便于审计与复算

> `uzi --preview` 用的 mock panel 是**简化版**（`panel_consensus = bullish/50*100`，
> 无共识元数据）。以真实 `--stage1` 产物为准。

## 9 大流派

| 组 | 流派 | 人数 |
|---|---|---|
| A | 经典价值 | 6 |
| B | 成长投资 | 9 |
| C | 宏观对冲 | 7 |
| D | 技术趋势 | 4 |
| E | 中国价投 | 7 |
| F | A股游资 | 24 |
| G | 量化系统 | 4 |
| H | 科技领袖 | 4 |
| I | AI 卡位猎手（Serenity） | 1 |

`school_scores` 按组给出 `consensus` / `avg_score` / `n_members` / `n_active` / `short_excluded`，
用于判断"是全体分歧还是某一派独撑"。

## Confidence 校准规则

| 区间 | 含义 |
|---|---|
| 85–100 | 核心方法论硬指标全部命中或全部不命中 |
| 60–84 | 多数命中 |
| 30–59 | 部分命中、需要等待信号 |
| 0–29 | 方法论不适用此股 / 信息不足 |

## 游资射程预过滤（F 组）

24 位游资先经 `seat_db::is_in_range(nickname, ticker_features)` 判断是否在射程内：

| 情况 | 结果 |
|---|---|
| 在射程 | 正常评分 |
| 不在射程 | `signal: "skip"`、`verdict: "不适合"`、`confidence: 0`、`skip_reason: "市值 N 亿不在 X 射程"` |
| 不在射程 **但 LHB 命中该席位** | **强制参与评分**（v3.4.5 覆盖） |

> 上游 SKILL.md 的旧描述说不在射程是 `neutral` / `confidence: 90`；**当前上游代码
> 与其自身文档不一致** —— 实测上游 `_skip_result` 与本移植同样输出 `skip` / `0`。
> 以代码与实际产物为准。

无显式 `max_mcap` 的游资有一条隐含上界（500 亿元）；`章盟主` 在白名单内，不受该上界约束。

实测 600519.SH（市值 1.59 万亿）F 组 24 人里 **22 人 skip**、仅 2 人评分 —— 这就是
`is_in_range` 在起作用，也说明"这只票有游资参与"这类结论必须先过射程判断。
详见 [`../lhb-analyzer/SKILL.md`](../lhb-analyzer/SKILL.md)。

## Agent 的职责（🧠 你）

`--stage1` 产出的 `headline` / `reasoning` 是**规则引擎的机械输出**。你要：

1. 读 `panel.json` 的 66 份骨架分，先看 `consensus_valid` / `hollow_pct`
   —— 共识无效时**必须**在报告里点明，而不是照抄一个不可信的数字
2. 按流派分组 role-play，用你的判断覆盖 `headline` / `reasoning` / `score`
3. 定性结论写进 `agent_analysis.json` 的 `panel_insights`（≥30 字，含投票分布 + 多空分歧）

**语言风格守则** —— `comment` 必须像本人：

- 巴菲特：温和、引用奥马哈、用"我们"
- 芒格：刻薄、反向思维、引用心理学偏误
- 索罗斯：哲学化、提"反身性"
- 章盟主：豪迈、提"格局"、不谈细节
- 赵老哥：直接、谈"题材"、谈"二板"
- 段永平：朴素、问"商业模式""人""价格"

若 `skills/deep-analysis/personas/{id}.yaml` 存在（本仓库随附 51 份），**flagship persona
的 YAML 优先于规则引擎**：headline 必须引用其 `key_metrics` 具体条目，reasoning 必须带
`voice` 字段的风格词，且 **signal 必须与其历史立场对齐**（巴菲特不会对 PE 882 的股票说买入）。

> `dalio` / `soros` / `fisher` 的判据字段名不同（`key_framework` / `key_signals` / 无），
> 需读原始 YAML —— 见 [`../deep-analysis/SKILL.md`](../deep-analysis/SKILL.md)。

> `skills/deep-analysis/personas/` 相对 cwd 解析 —— 从仓库根目录运行，或
> `export UZI_PERSONAS_DIR=<repo>/skills/deep-analysis/personas`。

## 锁定单一流派

`uzi <ticker> --school F` 会设置 `UZI_SCHOOL`，非该派评委被标 `signal="skip"`、
`reason="用户锁定 X 派视角"`。此时**只 role-play 该派**，`panel_insights` 只讨论派内分歧。
详见 [`../deep-analysis/SKILL.md`](../deep-analysis/SKILL.md) 的 HARD-GATE-SCHOOL-LOCK。

## 完成检查

- [ ] `investors` 含 66 份 Signal，字段齐全
- [ ] `signal_distribution` / `vote_distribution` 之和 = 评委人数 − `short_excluded`
- [ ] 已查看 `consensus_valid`：为 `false` 时报告必须提示共识不可信
- [ ] F 组不在射程的游资已标 `不适合`
- [ ] `agent_analysis.json.panel_insights` ≥ 30 字，写明分歧而非只给结论
