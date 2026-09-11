---
name: dcf
description: 对指定股票做机构级 DCF 估值 — WACC + 两段 FCF + 终值 + 5×5 敏感性表
---

# /dcf <股票代码或名称>

对目标股票做完整的 DCF 估值，按照 JPMorgan / Goldman / Morgan Stanley 投行标准。

## 工作流

1. 解析股票代码（`002273.SZ` / `00700.HK` / `AAPL` / 中文名）——`uzi` 自动识别。
2. 确保缓存就绪：DCF 属于 Task 1.5 机构建模（本项目 22 维体系中的 **dim 20**），
   随 `--stage1` 一起算好，落在
   `.cache/<ticker>/raw_data.json` 的 `dimensions["20_valuation_models"].data.dcf`。
   已有缓存可直接跳到第 3 步：

   ```bash
   uzi <ticker> --stage1
   ```

3. 取 DCF 结果（stdout 是纯 JSON，可直接 `jq`）：

   ```bash
   uzi <ticker> --method dcf
   ```

   或离线读缓存：

   ```bash
   jq '.dimensions["20_valuation_models"].data.dcf' .cache/<ticker>/raw_data.json
   ```

4. 展示结果（字段与上游 DCF 模型一一对应）：
   - WACC 分解 `wacc_breakdown`：`wacc` / `cost_of_equity` / `after_tax_kd` / `equity_weight` / `debt_weight`
   - 10 年 FCF 预测 `projected_fcf_yi`（显式期 5 + 过渡期 5），`year_labels` 对齐年份
   - 折现 `pv_fcf_yi` / `pv_explicit_yi`，终值 `terminal_value_yi` / `tv_pv_yi` / `tv_pct_of_ev`
   - EV → 净债 → 股权 → 每股内在价值：`enterprise_value_yi` → `net_debt_yi` → `equity_value_yi` → `intrinsic_per_share`
   - 安全边际 `safety_margin_pct` 和结论 `verdict`（深度低估 / 合理 / 高估）
   - **5×5 敏感性表** `sensitivity_table`（WACC ±200bp × 终值 g ±100bp），中心格等于基础案例

## A 股默认参数

上游 DCF 模型可传入的假设参数在这里是**固定默认值**，随结果原样输出到 `assumptions` 字段。
本项目 CLI **不暴露自定义假设入口**——要知道当前生效值：

```bash
jq '.dimensions["20_valuation_models"].data.dcf.assumptions' .cache/<ticker>/raw_data.json
```

| 字段 | 默认 | 如何影响结果 |
|---|---|---|
| `stage1_growth` | 0.10 | 显式期 5 年 FCF 增速 → 抬高/压低 `projected_fcf_yi` |
| `stage2_growth` | 0.05 | 过渡期 5 年 FCF 增速 → 终值前的现金流水平 |
| `stage1_years` / `stage2_years` | 5 / 5 | 两段年限加起来构成 10 年预测 |
| `terminal_g` | 0.025（终值永续） | 终值增长率 → `terminal_value_yi` 与敏感性表列 |
| `target_debt_ratio` | 0.30 | 目标资本结构 → `wacc_breakdown` 权重 → 全表折现率 |
| `beta` | 1.0 | CAPM `cost_of_equity` → `wacc` |
| `tax` | 0.25（高新 15%） | 税后债务成本 `after_tax_kd` → `wacc` |

无风险利率固定 2.5%（10Y 国债）、股权风险溢价固定 6%，见 `wacc_breakdown.inputs`。

## 完成检查

```bash
jq '.dimensions["20_valuation_models"].data.dcf
  | {intrinsic_per_share, safety_margin_pct, verdict, assumptions}' .cache/<ticker>/raw_data.json
```

- `intrinsic_per_share` 非 `null`；为 `null` 时 `error` 字段给出数据不足原因（FCF / 营收 / 净利率均缺）。
- `sensitivity_table` 中心格等于基础案例每股内在价值。
- `assumptions` 存在，且为上述默认口径（无自定义覆盖）。

## 方法论参考

`skills/deep-analysis/references/fin-methods/README.md`
