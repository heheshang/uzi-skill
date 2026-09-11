---
name: comps
description: 同行对标相对估值 — PE/PB/EV-EBITDA 分位分析 + 隐含目标价
---

# /comps <股票代码>

对目标股票做机构级 Comparable Company Analysis，识别相对同行的估值位置。

## 工作流

1. 目标股票与同行池都由 `--stage1` 一并抓取——本项目同行直接来自
   `dimensions["4_peers"]`，无需单独抓取。已有缓存可跳到第 2 步：

   ```bash
   uzi <ticker> --stage1
   ```

2. 取可比公司表（stdout 是纯 JSON，可直接 `jq`）：

   ```bash
   uzi <ticker> --method comps
   ```

   或离线读缓存：

   ```bash
   jq '.dimensions["20_valuation_models"].data.comps' .cache/<ticker>/raw_data.json
   ```

3. 输出：
   - 同行池 `peers` / `peer_count`（默认 4-10 家，已剔除目标公司自身；少于 2 家会跳过对标）
   - 关键倍数统计 `peer_stats.<metric>`：`min` / `p25` / `median` / `p75` / `max` / `mean` / `n`
   - 目标公司在每个倍数上的百分位排名 `target_percentile.<metric>`
   - 中位 PE × EPS → 隐含每股价 `implied_price.via_median_pe`
   - 中位 PB × BVPS → 隐含每股价 `implied_price.via_median_pb`
   - 估值结论 `valuation_verdict`（便宜 / 合理偏低 / 合理偏高 / 昂贵）

   覆盖的倍数字段：`pe` / `pb` / `ps` / `ev_ebitda` / `ev_sales` / `roe` / `net_margin` / `revenue_growth`。

## 展示规范

- 峰值排序：PE / EV-EBITDA / P/S 三栏为主
- 颜色：低于 p25 标绿，高于 p75 标红
- 百分位：0-25 便宜 / 25-50 合理偏低 / 50-75 合理偏高 / 75-100 昂贵

## 完成检查

- `peer_count ≥ 2`，否则 `valuation_verdict` 为「同行样本不足 · 无法对标」。
- `implied_price` 至少含一条 `via_median_pe` / `via_median_pb`。
- `target_percentile.pe` 与 `valuation_verdict` 档位一致（≤25 便宜 … >75 昂贵）。
