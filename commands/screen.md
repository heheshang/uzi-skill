---
name: screen
description: 量化筛选 — A+港股每日筛选（游资 F 组 + Serenity 角色组）与单股经典 screen
---

# /screen 每日筛选

## 每日全市场筛选

A+港股每日筛选（`--screen daily`），按 F 组游资 + I 组 Serenity 视角出「每日观察榜」：

```bash
uzi --screen daily
```

已有 flag：

| flag | 默认 | 说明 |
|---|---|---|
| `--mode noon\|close` | `noon` | 午间 / 收盘快照口径 |
| `--markets A,H` | `A,H` | 逗号分隔的市场 |
| `--schools F,I` | `F,I` | 角色组，当前固定 F,I |
| `--top 10` | `10` | 每日候选上限，最多 10 只（不凑数） |
| `--min-turnover 2e8` | `2e8` | 最低实际成交额（本币） |
| `--snapshot-only` | 关 | 仅用行情横截面，不抓逐股增强 |
| `--output-dir <DIR>` | — | 把产出拷到指定目录并生成 `index.html` / `report.meta.json` |

示例：

```bash
uzi --screen daily --mode close --markets A,H --top 10
uzi --screen daily --snapshot-only --markets A
```

输出：`reports/screens/<日期>/<report_id>/index.html` —— 每条候选含条件通过 / 等回踩 /
可买动作与证据时间戳；`--snapshot-only` 的候选会被标记为快照观测、不做增强。

## 单股经典 screen

单股版的 value / growth / quality / gulp 四套经典 quant screen 走研究工作流（dim 21）：
先 `uzi <ticker> --stage1`，再读 `--method idea-screen`：

```bash
uzi <ticker> --stage1
uzi <ticker> --method idea-screen
```

| Style | 核心标准 |
|---|---|
| `value` | PE<15 · PB<1.5 · 股息率>3% · FCF>0 · 负债率<50% |
| `growth` | 营收增速>15% · 净利增速>20% · 毛利扩张 · ROE>15% |
| `quality` | ROE 5Y>15% · 净利率>15% · FCF+ · 债务<50% · 护城河≥28 |
| `gulp` | PEG<1.5 · 营收>15% · ROE>15% · Stage 2 |

## 输出

- 每条标准 pass / fail 列表
- `passed / total` 命中率
- `pass_rate_pct ≥ 70%` → 🟢 命中筛选
- `fits_screen` 布尔标记
