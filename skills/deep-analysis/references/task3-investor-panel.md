# Task 3 · 66 贤评审团

Stage 1 载入 `investor-panel` 评审团，让 66 位投资大佬各自按方法论给出 Signal：

```bash
uzi <ticker> --stage1      # 产出 .cache/{ticker}/panel.json（Task 3）
```

> 只要投票、不想要整份报告时：仍走 `--stage1`，然后直接读 `panel.json`，不必跑 `--stage2`。

## Signal 模式（抄自 ai-hedge-fund）

每位投资者必须输出严格三元组：
```json
{
  "investor_id": "buffett",
  "name": "巴菲特",
  "group": "A",
  "mandate": "long",
  "signal": "bullish",          // bullish | bearish | neutral | skip
  "confidence": 87,             // 0-100，越高越坚定
  "score": 82,                  // 0-100，综合评分（沿用原方案，便于排序）
  "verdict": "买入",            // 强烈买入/买入/关注/观望/等待/回避/不达标/不适合
  "headline": "一句话结论",
  "reasoning": "ROE 五年>18%，护城河清晰，但价格已经不便宜，等回调...",
  "comment": "用该投资者语言风格的金句，1-2 句",
  "pass": ["ROE>15% 连续 5 年", "净利率 22%"],
  "fail": ["PE 已超历史 70 分位"],
  "ideal_price": 16.20,         // 理想买入价（如适用）
  "period": "3-5 年",           // 建议持仓周期
  "skip_reason": null           // 仅 signal=skip 时给出（如射程外）
}
```

字段全集与顶层结构以 [`../investor-panel/SKILL.md`](../../investor-panel/SKILL.md) 为准
（含 `short_consensus` / `consensus_formula` / `school_scores` 等）。

## 9 大流派分组

| 组 | 名称 | 人数 | 规则来源 |
|---|---|---|---|
| A | 经典价值派 | 6 | `uzi_investors::criteria` |
| B | 成长投资派 | 9 | `uzi_investors::criteria` |
| C | 宏观对冲派 | 7 | `uzi_investors::criteria` |
| D | 技术趋势派 | 4 | `uzi_investors::criteria` |
| E | 中国价投/公募派 | 7 | `uzi_investors::criteria` |
| F | A 股游资派 | 24 | `uzi_investors::seat_db` |
| G | 量化系统派 | 4 | `uzi_investors::criteria` |
| H | 科技领袖派 | 4 | `uzi_investors::criteria` |
| I | AI 卡位猎手（Serenity） | 1 | `uzi_investors::criteria` |
| **共** | | **66** | |

> 名册内嵌于 `crates/uzi-investors/src/data/investors.json`（`id` / `name` / `group` / `fields` 白名单 / `mandate`）；
> F 组含章盟主、赵老哥、佛山无影脚、北京炒家等 24 位；席位与射程规则内嵌于 `crates/uzi-investors/src/data/seats.json`。

## 字段白名单（per-persona 抄 ai-hedge-fund）

每位投资者只看自己关心的维度，避免噪音（白名单在 `uzi_investors::criteria`）：

```rust
// persona id → 允许读取的维度 key
buffett   => ["1_financials", "10_valuation", "11_governance", "14_moat"],
graham    => ["1_financials", "10_valuation"],
lynch     => ["1_financials", "7_industry", "10_valuation"],
minervini => ["2_kline", "16_lhb"],
youzi.*   => ["2_kline", "12_capital_flow", "15_events", "16_lhb", "17_sentiment"],
trap      => ["18_trap"],
```

游资统一不看财报（除小鳄鱼），只看 K线+资金+龙虎榜+情绪。

## 执行流程

1. `uzi <ticker> --stage1` 产出骨架：白名单来自 `uzi_investors::criteria`，
   评分由 `uzi_investors::evaluator` 完成，面板汇总/共识由 `uzi_pipeline::panel` 完成
2. 对每位投资者：
   a. 取出该 persona 的字段白名单
   b. 从 `dimensions.json` + `raw_data.json` 提取相关字段
   c. 用该 persona 的方法论 + 语言样本生成 Signal（语言风格见 `uzi_investors::personas`；
      flagship persona 若存在 `skills/deep-analysis/personas/{id}.yaml`，`uzi_investors::persona_yaml` 优先于规则引擎）
   d. 强制 JSON 输出
3. 汇总到 `panel.json`：
```json
{
  "ticker": "600519.SH",
  "panel_consensus": 64.2,          // 多头共识分，公式见下
  "consensus_valid": true,          // hollow_pct < 20 才为 true
  "signal_distribution": {
    "bullish": 20, "neutral": 14, "bearish": 7, "skip": 23
  },
  "vote_distribution": {
    "strongly_buy": 6, "buy": 9, "watch": 12, "wait": 10, "avoid": 3, "n_a": 1, "skip": 23
  },
  "short_consensus": { "total": 2, "active": 1, "skip": 1, "short_candidates": 1 },
  "investors": [ {Signal}, {Signal}, ... ]   // 66 个
}
```

> ⚠️ `signal_distribution` / `vote_distribution` 只统计**多头簿**，不含 `mandate == "short"`
> 的评委（他们进 `short_consensus`，并记入 `consensus_formula.short_excluded`）。
> 上例四桶之和 = 64 = 66 − 2，**不要**把"四桶之和 = 评委人数"当作校验条件。

### 共识公式（v2.15.5）

```
consensus_raw   = 0.65 * score_mean + 0.35 * vote_weighted
panel_consensus = polarize(consensus_raw, k = 1.3)

neutral_weight  = 0.6      # 中性票半权计入投票部分
```

`consensus_formula` 字段回写 `score_mean` / `vote_weighted` / `consensus_raw` / `consensus_final`
与 `bullish` / `neutral_weighted` / `bearish` / `skip` / `active` / `short_excluded`，便于审计复算。

## 重要：游资是否在射程内

24 位游资，**不是每只票都适合每位游资**。`uzi_investors::seat_db` 的
`is_in_range(nickname, ticker_features)` 先做射程预过滤：

| 游资 | 适合的票 | 不在射程则 |
|---|---|---|
| 章盟主 | 市值 > 200 亿 + 趋势向上 | `signal: "skip"` · `verdict: "不适合"` · `confidence: 0` · `skip_reason: "市值 N 亿不在 X 射程"` |
| 赵老哥 | 板块辨识度龙头 + 连板潜力 | 同上 |
| 佛山无影脚 | 小盘 + 超跌 | 同上 |
| 北京炒家 | 20-80 亿 + 题材 + 机构持仓 < 10% | 同上 |

> ⚠️ 不在射程**不是** `neutral` / `confidence: 90`。当前实现输出 `skip` + `confidence: 0`，
> 上游文档的旧描述与其自身代码也不一致 —— 以 `panel.json` 实际产物为准。

**例外**：不在射程**但龙虎榜命中该席位**（`uzi_data::fetch::lhb`）→ **强制参与评分**（v3.4.5 覆盖）。
无显式 `max_mcap` 的游资有一条隐含上界（500 亿元）；`章盟主` 在白名单内，不受该上界约束。

完成后向用户汇报：`Task 3 ✓ 66 位评审完成，看多 X / 中性 Y / 看空 Z（skip N）`。
