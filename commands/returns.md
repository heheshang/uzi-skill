---
name: returns
description: 组合收益归因 — 拆解总收益为各持仓贡献 + 行业/流派归因 + Top 贡献/拖累 + vs 基准
---

# /returns <holdings.csv | 组合>

把一个组合的**区间总收益**拆开：谁赚的钱、哪类资产赚的、谁是英雄谁是猪队友。
本方法负责"涨跌从哪来"；组合体检（打分 / 健康度）由 `uzi --portfolio <csv>` 负责。

> 改编自 anthropics/financial-services · private-equity/returns-analysis，适配二级市场组合。
> 方法详见 `skills/deep-analysis/references/fin-methods/returns-attribution.md`。

## 输入

```bash
uzi --portfolio <csv> --method returns
```

组合 CSV 只有三列会被读取：`ticker` / `weight` / `note`（与 `uzi --portfolio` 同款，
解析器 `uzi_screen::portfolio::parse_csv` 只产出这三个字段）。

```
ticker,weight,note
600519.SH,0.40,白酒
000858.SZ,0.30,白酒
002594.SZ,0.30,电动车
```

- `weight` 0-1 或 0-100 都行，自动归一化；缺失则等权。
- **`return_pct` / `industry` / `name` 由 CLI 自动从缓存补齐**，不必（也无法）写在 CSV 里：

  | 字段 | 来源 |
  |---|---|
  | `return_pct` | `.cache/<ticker>/raw_data.json` → `2_kline.kline_stats.ytd_return` |
  | `industry` | 同上 → `0_basic.data.industry` |
  | `name` | 同上 → `0_basic.data.name` |

  因此**每只持仓都要先跑过 `uzi <ticker> --stage1`**，否则该只会被标
  「需补价格区间」并按 0 计入 —— 这是有意的：宁可显式报缺，也不编一个收益率。
- `school`（流派归因）CLI 不补齐（CSV 只有三列），需要时由源码侧构造 holdings 传入库函数
  `uzi_models::tier1::returns_attrib`。
- **benchmark 当前 CLI 未暴露为 flag**：`--method returns` 按 `benchmark=None` 调用，输出的
  `benchmark` 字段为 `null`，此时**不做基准超额对比**（`verdict` 只报组合自身收益）。
  需要基准时由源码侧调用同一个库函数显式传参（CLI 不暴露该 flag）。

## 工作流

```bash
uzi --portfolio holdings.csv                    # 组合体检（加权评分 + 集中度）
uzi --portfolio holdings.csv --method returns   # 按需计算收益归因（stdout 纯 JSON）
```

## 输出

| 模块 | 内容 |
|---|---|
| **总收益** | `total_return` = Σ(权重×个股收益)，单位 pp |
| **加权贡献表** | 逐持仓：仓位 / 收益 / 贡献(pp) / 是否需补价 |
| **行业归因** | 各行业贡献（降序），加总 == 总收益 |
| **流派归因** | 仅当 holdings 带 `school` |
| **Top3 贡献 / Top3 拖累** | 正/负贡献排序 |
| **vs 基准** | 超额 = 总收益 − 基准，跑赢/跑输 |
| **一句话点评** | `verdict` |

## 展示示例

```
组合区间总收益 +8.50%（vs 基准 +6.00% · 超额 +2.50pp · 🟢 跑赢）

加权贡献表：
  比亚迪    电动车  30%  +20.0%  贡献 +6.00pp  ←主升
  贵州茅台   白酒   40%  +10.0%  贡献 +4.00pp
  五粮液    白酒   30%   -5.0%  贡献 -1.50pp  ✕拖累

行业归因：电动车 +6.00pp · 白酒 +2.50pp
Top 贡献：比亚迪 +6.00 / 贵州茅台 +4.00
Top 拖累：五粮液 -1.50

一句话：组合总收益 +8.50%，主升由比亚迪贡献 +6.00pp，主要拖累五粮液 -1.50pp，跑赢基准 +2.50pp。
```

## 注意

- 贡献单位是**百分点 (pp)**（已乘权重），不是个股收益率本身。
- 分组归因只是重排，加总必须等于总收益。
- 缺 `return_pct` 不报错，但 verdict 会提示"⚠️ N 只缺区间收益需补价格"。
- 实绩归因（已发生区间收益），不做情景/敏感性预测。
