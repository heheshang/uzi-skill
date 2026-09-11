---
name: initiate
description: 生成机构风格首次覆盖报告 — Executive Summary + 投资论点 + 估值桥 + 风险
---

# /initiate <股票代码>

按照 JPMorgan / Goldman / Morgan Stanley 首次覆盖报告格式，生成一份完整的 initiating coverage。

## 工作流

1. 首次覆盖报告由 Task 1.5 研究工作流产出，落在
   `dimensions["21_research_workflow"].data.initiating_coverage`；其估值桥所需的 dim 20
   的 DCF + Comps 结果由管线自动喂入。确保缓存就绪：

   ```bash
   uzi <ticker> --stage1
   ```

2. 取报告（stdout 是纯 JSON，可直接 `jq`）：

   ```bash
   uzi <ticker> --method initiate
   ```

   或离线读缓存：

   ```bash
   jq '.dimensions["21_research_workflow"].data.initiating_coverage' .cache/<ticker>/raw_data.json
   ```

3. 输出六个标准章节：
   - **I. Executive Summary** `executive_summary` + `headline` — 推荐评级 `rating` / 目标价 `target_price` / 上行空间 `upside_pct`
   - **II. Investment Thesis** `investment_thesis` — 3-5 条核心看多支柱
   - **III. Valuation Bridge** `valuation_bridge` — DCF + Comps + Blended
   - **IV. Key Risks** `key_risks` — 风险 + 严重度
   - **V. Financial Snapshot** `financial_snapshot` — 5 年历史 ROE/营收/净利
   - **VI. Coverage Universe Positioning** `coverage_universe_pos` — 分析师覆盖密度

## 评级规则

- Upside ≥ 25% → 买入 (Overweight)
- 10-25% → 增持 (Outperform)
- -10-10% → 持有 (Neutral)
- < -10% → 减持 (Underperform)

无有效目标价时评为「未评级 (Not Rated)」。

## 完成检查

- `headline.rating` / `headline.target_price` / `headline.upside_pct` 与评级规则档位一致。
- `valuation_bridge` 含 DCF（如有）+ Comps 各项 + `Blended`；无有效目标价时评级为「未评级」。
- `financial_snapshot` 的 `roe_history` / `revenue_history_yi` / `net_profit_history_yi` 各至多 5 期。
