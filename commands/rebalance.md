---
name: rebalance
description: 组合再平衡 — 漂移检测 + 交易清单 + 换手成本（A股/港股/美股，无 TLH）
---

# /rebalance <持仓.csv>

对一个组合做**再平衡分析**：当前权重偏离目标多少、要不要动、动的话买卖什么、换手要花多少钱。

A 股个人无资本利得税 → **不做税损收割 (TLH)**，聚焦 **漂移 + 风险 + 换手成本**。
（持仓含美股时，会一句话提示"美股部分可另议税损"。）

## 输入

```bash
uzi --portfolio <csv> --method rebalance
```

`--portfolio` 的 CSV 基础列为 `ticker, weight, note`（解析器 `uzi_screen::portfolio::parse_csv`
只产出这三个字段）。其余字段由 CLI **从缓存补齐**，不必（也无法）写在 CSV 里：

| 字段 | CSV | 说明 |
|---|---|---|
| `ticker` | ✅ 必填 | `600519.SH` / `00700.HK` / `AAPL` |
| `weight` | ✅ 必填 | 当前权重，0-1 小数或 0-100 百分数（自动归一化） |
| `note` | 选 | 备注 |
| `industry` | ❌ | CLI 从 `0_basic.data.industry` 补；用于行业分散度 |
| `price` | ❌ | CLI 从 `0_basic.data.price` 补；用于估股数（A 股按手取整） |
| `market` | ❌ | 由 `uzi_models::tier1::rebalance::infer_market` 按 ticker 后缀推断 |
| `value` | ❌ | **无法提供** —— 需要组合总市值，缓存里没有；缺省只算权重口径 |

> 补齐前提：每只持仓先跑过 `uzi <ticker> --stage1`。未缓存的持仓不会被编造数据。

- **targets / threshold 当前 CLI 未暴露为 flag**：运行时目标权重默认**等权 (1/N)**、漂移阈值默认 **5pp**（偏离 >5pp 才动）。自定义目标权重属于源码侧用法 —— CLI 不提供该入口，默认等权口径下无需它。

## 工作流

```bash
uzi --portfolio holdings.csv                      # 组合体检（加权评分 + 集中度）
uzi --portfolio holdings.csv --method rebalance   # 漂移 + 调仓清单 + 换手成本
```

先用同一份 holdings 跑组合体检，再拿同样的 CSV 做调仓建议。

## 输出

1. **漂移表** `drift_table` — 每只 当前 vs 目标 vs 漂移(pp) + 是否超阈值 + 方向（超配→卖/低配→买）
2. **阈值判断** `summary.any_breach` / `n_breached` / `max_drift_pp` — 默认 >5pp 才触发
3. **交易清单** `trades` — 仅对超阈值持仓：BUY/SELL + 金额 + 估算股数（A 股按手）
4. **换手成本** `turnover_cost` — 分市场拆解：
   - A 股：卖出印花税 0.05%（2023-08 下调）+ 双边佣金 ~0.025%
   - 港股：印花税 0.1%（双边）+ 佣金
   - 美股：印花税 ≈ 0
5. **集中度变化** `concentration` — 前 3 大集中度 / 最大单只 / HHI / 行业分散 的 前→后 对比

## 注意

- 小幅漂移在阈值内不动，别为再平衡而再平衡。
- 没给 `value/price` 时只出漂移方向，不估金额与成本。
- 不做 TLH（A/港股个人无资本利得税）；美股持仓的税损另议。

> 改编自 `anthropics/financial-services` wealth-management/portfolio-rebalance，A 股适配。
> 方法论详见 `skills/deep-analysis/references/fin-methods/rebalance.md`。
