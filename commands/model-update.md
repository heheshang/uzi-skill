---
name: model-update
description: 增量更新财务模型 — 用新财报/新指引/修正假设重算 DCF 内在价值、Comps 隐含价、投资逻辑与目标价，输出 before→after delta
---

# /model-update <股票代码>

财报发布 / 公司更新指引 / 修正假设后，**基于当前缓存数据重算**已有财务模型，
而不是从头重算。输出关键假设 before→after 的 delta 表，并把改动传导到
DCF 内在价值、Comps 隐含价、投资逻辑各支柱，给出更新后的 verdict。

> **本项目差异**：`--method model-update` **不接收外部 update payload**（CLI 未暴露 `updates` 入参）。
> 它以缓存 `.cache/<ticker>/raw_data.json` 的当前 features 为唯一输入，自动推断「最新 vs 上期」出 delta；
> 要改假设需先刷新缓存数据再重算，不能通过命令行直接传入新假设。

A 股 / 港股 / 美股通用；估值参数沿用 UZI（A 股 rf 2.5% / ERP 6% / 税 25%）。

触发词：「更新模型 / 改假设重算 / 新指引 / 财报更新数字 / 修正估计 / revise estimates」。

## 输入

| 来源 | 说明 |
|---|---|
| 缓存 features | `--stage1` 产出的 `raw_data.json`（营收 / 毛利率 / 净利率 / capex 的最新实绩与上期值） |
| 新财报实绩 | 有新报表时先用 `--no-resume --stage1` 全量重抓刷新缓存，再重算 |
| DCF / Comps 结果 | 估值影响段取自缓存里已算好的 DCF / Comps 结果；缺失则该段标记「未提供」 |

模型内部重算的假设键：`rev_growth` · `gross_margin` · `net_margin` · `capex_pct` ·
`stage1_growth` · `terminal_g` · `beta` · `target_pe` · `target_price`。
**这些键当前 CLI 不接受外部覆盖**，仅由缓存 + 模型推断，列出供理解 delta 表口径。

## 工作流

1. 刷新 / 复用缓存 features：
   ```bash
   uzi <ticker> --stage1                 # 复用缓存
   uzi <ticker> --no-resume --stage1     # 忽略缓存全量重抓（有新财报时）
   ```
2. 需要单独查看估值影响基准时（已跑过则复用缓存，无需重复）：
   ```bash
   uzi <ticker> --method dcf
   uzi <ticker> --method comps
   ```
3. 基于当前缓存重算模型：
   ```bash
   uzi <ticker> --method model-update
   ```
   - 缓存无 DCF / Comps 段 → 对应影响段标记「未提供」，结构照常返回。

## 输出

- **① 假设 delta 表**：每条假设 before → after（↑/↓/→）+ 影响通道。
- **② DCF 内在价值影响**：每股内在价值 before→after、delta%、WACC 变化、安全边际变化。
- **③ Comps 隐含价影响**：中位 PE × EPS 隐含价 before→after、delta%。
- **④ 投资逻辑影响**：每条改动映射到对应支柱（成长性/盈利质量/现金流/估值锚），💪 强化 / ⚠️ 削弱。
- **⑤ 更新后 verdict**：综合分 → 上修 / 维持 / 下修 + 建议动作。

## 展示示例

```
📊 模型更新 · 测试科技 (000001.SZ) · 缓存重算模式

① 关键假设 delta
  营收增速     15.0% → 26.0%  (↑)  [→DCF]
  净利率       14.0% → 16.0%  (↑)  [→DCF/Comps]
  Capex/营收    6.7% →  7.0%  (↑)  [→DCF]

② DCF 内在价值   ¥20.00 → ¥23.05  (+15.3%)  ↑
   WACC 8.50% → 8.50% · 安全边际 +8.1% → +24.6%

③ Comps 隐含价   ¥22.00 → ¥25.15  (+14.3%)  ↑（净利率放大 EPS）

④ 投资逻辑
  成长性（营收增速）   15.0%→26.0% (↑)   💪 强化
  盈利质量（净利率）   14.0%→16.0% (↑)   💪 强化
  现金流/资本纪律      6.7%→7.0%  (↑)   ⚠️ 削弱

⑤ 更新后评级：🟢 上修 (Upgrade)（综合分 +31.5）→ 上调目标价 / 加仓候选
```

## 方法论

详见 `skills/deep-analysis/references/fin-methods/model-update.md`
（改编自 anthropics/financial-services equity-research/model-update）。
