---
name: lbo
description: LBO 快速测试 — 假设 PE 买方用 5x 杠杆能否赚 20%+ IRR
---

# /lbo <股票代码>

作为估值交叉验证，测试"如果 PE 基金今天按市场价买入并 5 年后退出，能否赚到 20%+ IRR？"

## 工作流

1. LBO 属于 Task 1.5 机构建模（dim 20），随 `--stage1` 一起算出。确保缓存就绪：

   ```bash
   uzi <ticker> --stage1
   ```

2. 取 LBO 结果（stdout 是纯 JSON，可直接 `jq`）：

   ```bash
   uzi <ticker> --method lbo
   ```

   或离线读缓存：

   ```bash
   jq '.dimensions["20_valuation_models"].data.lbo' .cache/<ticker>/raw_data.json
   ```

## 假设

上游 LBO 模型可传入的参数（入场/退出倍数、杠杆倍数、持有年数、EBITDA 增速、利率）
在这里是**固定默认值**，本项目 CLI **不暴露自定义参数入口**。当前生效值体现在输出的
`entry_multiple` / `leverage_turns` / `exit_multiple` 字段（默认 8x / 5x / 8x）；
EBITDA 路径看 `ebitda_path`，还债进度看 `debt_schedule`。

## 输出

- **入场**：EBITDA `entry_ebitda_yi` × 8x → EV `entry_ev_yi` → 债务 `entry_debt_yi` (5x) + 股权 `entry_equity_yi`
- **5 年 EBITDA 路径** `ebitda_path`
- **债务偿还进度** `debt_schedule`（FCF ≈ EBITDA × 50%，其中 70% 用于还债）
- **退出**：Y5 EBITDA `exit_ebitda_yi` × 8x → EV `exit_ev_yi` − 剩余债 = 股权 `exit_equity_yi`
- **IRR / MOIC**：`irr_pct` / `moic` / `pass_pe_test`
- **verdict** `verdict`：
  - 🟢 IRR ≥ 20% — PE 可赚
  - 🟡 15-20% — 边际
  - 🔴 < 15% — PE 不会买

## 为什么重要

LBO 测试是"私募买方视角的估值下限"。如果一个股票连 PE 基金用杠杆都赚不到钱，说明市场定价已经很贵了。这是对 DCF 的独立交叉校验。

## 完成检查

- `irr_pct` 与 `pass_pe_test` 一致（IRR ≥ 20% → `true`）。
- `debt_schedule` 长度为 `hold_years + 1`（1 条期初 + 每年 1 条）。
- `verdict` 与 `irr_pct` 档位一致（<15 / 15-20 / ≥20）。
