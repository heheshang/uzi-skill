---
name: lhb-analyzer
description: 龙虎榜深度分析器。识别游资席位、判断机构 vs 游资博弈、对照同板块龙虎榜找辨识度龙头。当用户问"谁在买这只票/最近龙虎榜怎么样/X游资有没有上榜/这是不是X的票"时使用。
version: 3.9.4
author: FloatFu-true
license: MIT
metadata:
  tags: [finance, a-share, lhb, hot-money, market-microstructure]
  related_skills: [deep-analysis]
---

# LHB Analyzer · 龙虎榜分析

## 📚 席位百科

[`references/seat-encyclopedia.md`](references/seat-encyclopedia.md) —— 23 个席位的真名、tier、
风格、席位别名与 `fit_rules`（市值/换手/基本面/趋势门槛），按本项目内嵌席位表
（源 `crates/uzi-investors/src/data/seats.json`）校正过。

判断"是不是 X 的票"、或写游资相关结论前先读它。

## 这个 skill 在本仓库的形态

龙虎榜是**维度 `16_lhb`**，随 Stage 1 一起产出。

| 上游 | 本仓库实现 |
|---|---|
| `fetch_lhb.py` | `crates/uzi-data/src/fetch/lhb.rs` |
| `lib/seat_db.py` | `crates/uzi-investors/src/seat_db.rs` + `src/data/seats.json` |
| `assets/seats-2026.json` | 已内嵌为 `crates/uzi-investors/src/data/seats.json`（23 席位） |

```bash
uzi <ticker> --stage1
jq '.dimensions["16_lhb"].data' .cache/<ticker>/raw_data.json
```

> **仅 A 股**。非 A 股返回 `{"_note": "lhb only A-share"}`、`source: "skip"`、`fallback: false`。

## 输出 · `16_lhb.data`

| 字段 | 含义 |
|---|---|
| `lhb_count_30d` | 近 30 日上榜次数 |
| `lhb_records` | 近 30 条上榜明细 |
| `matched_youzi` | 命中的游资席位昵称列表 |
| `matched_youzi_detail` | 每个席位的前 3 条明细 |
| `inst_vs_youzi` | 机构 vs 游资资金拆分（见下） |
| `sector_lhb_top50` | 同行业龙虎榜（最多 30 条） |
| `sector_leader_hint` | 同板块辨识度龙头代码 |

`inst_vs_youzi`：

```json
{
  "institutional_buy": 0.0,
  "institutional_sell": 0.0,
  "institutional_net": 0.0
}
```

机构 = 席位名含机构标识的营业部；其余归为游资。**净额为负且绝对值大**说明机构派发，
**游资买入但机构净卖** 是最典型的"游资接盘"形态。

## 席位识别

`seat_db::match_seats_in_lhb(records)` 把上榜营业部名称比对到 23 位游资席位表
（**运行时**不用调它：结果已在 `raw_data.json` → `16_lhb.data.matched_youzi` /
`matched_youzi_detail`）。
每个席位记录含：

```json
{
  "real_name": "...",
  "tier": "...",
  "style": "...",
  "premium": 0.0,
  "seats": ["营业部名称A", "营业部名称B"],
  "fit_rules": {"max_mcap": 0.0, "themes": ["..."]}
}
```

**同一游资常挂多个营业部** —— 判断"是不是 X 的票"要比对 `seats` 的**全部**别名，
不能只看昵称字面匹配。

## `is_in_range` —— 射程判断

`seat_db::is_in_range(nickname, ticker_features)` 判断某游资**是否可能参与**这只票：

- 有显式 `max_mcap` 时按其市值上限
- 无显式上限的游资走隐含上界 **500 亿元**
- `章盟主` 在白名单内，**不受**该上界约束（可参与超大盘）

**v3.4.5 覆盖规则**：即使算出"不在射程"，只要 `16_lhb.data.matched_youzi` 里命中了该席位，
仍**强制参与评分** —— 真实成交记录优先于静态射程假设。

这条是双向的：

- 射程外 **且** 未上榜 → 该游资对本票**无意见**（`signal: "skip"`），不要替他发言
- 射程外 **但** 上了榜 → 必须评分，且这是**比射程更有力**的参与证据

用途：**先过滤再下结论**。一只 9000 亿市值的票，绝大多数游资根本不在射程内，
写"赵老哥可能进场"是错的。

## 短线判断的硬性要求

1. **先席位匹配 + `is_in_range()`，再给短线判断**（见根 `SKILL.md` Agent 规则 5）
2. 结论必须落到**具体席位**与**具体日期**，不能只说"有游资进场"
3. 对照 `sector_lhb_top50` 判断**辨识度** —— 同板块多只上榜时，谁是龙头看资金体量而非涨幅
4. 机构净卖 + 游资净买 → 明确提示接盘风险

## 完成检查

- [ ] `matched_youzi` 是否为空都给出明确结论（不能略过）
- [ ] 提到的每个游资都做过 `is_in_range` 判断
- [ ] 已用 `sector_lhb_top50` 对照板块辨识度
- [ ] 机构 / 游资方向不一致时，把冲突写出来而不是取平均

## 相关

- 短线资金判断的方法论与 persona 风格：[`../investor-panel/SKILL.md`](../investor-panel/SKILL.md) F 组
- F 组评委的射程预过滤同样调用 `is_in_range`
