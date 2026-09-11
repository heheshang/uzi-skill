---
name: ic-memo
description: 投委会备忘录 — 8 章节结构化决策文档（含三情景回报 + Top 3 风险）
---

# /ic-memo <股票代码>

生成 PE/VC 风格的 Investment Committee Memo，适合正式决策。

## 工作流

1. IC Memo 由 Task 1.5 深度决策阶段产出（本项目 22 维体系中的 **dim 22**）。
   上游手动传入的 DCF / Comps / Porter / DD 前置结果，在本项目里由管线从
   dim 20/22 自动取好。确保缓存就绪：

   ```bash
   uzi <ticker> --stage1
   ```

2. 取备忘录（stdout 是纯 JSON，可直接 `jq`）：

   ```bash
   uzi <ticker> --method ic-memo
   ```

   或离线读缓存：

   ```bash
   jq '.dimensions["22_deep_methods"].data.ic_memo' .cache/<ticker>/raw_data.json
   ```

3. 输出 8 个章节，全部挂在 `sections` 下：

   - **I. Executive Summary** `sections.I_exec_summary` — `headline` 建议 + `recommendation` + 前 3 风险 `top_3_risks`
   - **II. Company Overview** `sections.II_company_overview` — 行业 / 业务 / 市值 / 营收
   - **III. Industry & Market** `sections.III_industry_market` — TAM / 增速 / 生命周期
   - **IV. Financial Analysis** `sections.IV_financial_analysis` — 5Y ROE / 净利率 / 债务 / FCF
   - **V. Valuation** `sections.V_valuation` — DCF + Comps 双路径
   - **VI. Risks & Mitigants** `sections.VI_risks_mitigants` — 主要风险 + 严重度 + 缓解措施
   - **VII. Returns Scenarios** `sections.VII_returns_scenarios` — Bull / Base / Bear 三情景（含概率）
   - **VIII. Recommendation** `sections.VIII_recommendation` — PASS / CONDITIONAL PASS / HOLD / REJECT

## 推荐逻辑

质量分 (ROE 持续性 + 现金流 + 护城河 + 净利率) + 估值分 (安全边际) → 综合得分决定建议。

## 完成检查

- `sections` 含全部 8 个键（`I_exec_summary` … `VIII_recommendation`）。
- `sections.I_exec_summary.recommendation` 非空。
- `sections.VI_risks_mitigants` 至少 1 条风险。
