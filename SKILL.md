---
name: uzi
description: A-share, Hong Kong, and US stock analysis skill for deep research, quick scans, investor panel review, hot-money/LHB analysis, trap detection, valuation, IC memos, and Bloomberg-style HTML reports. Rust implementation of UZI-Skill.
version: 3.9.4
author: FloatFu-true
license: MIT
metadata:
  tags: [finance, stocks, a-share, hong-kong, us-stocks, dcf, valuation, investor-panel, youzi, lhb, trap-detection]
  related_skills: [deep-analysis, investor-panel, lhb-analyzer, trap-detector]
---

# UZI Skill Root

单二进制 **`uzi`** 的顶层入口。四个 skill 共用同一套数据层（`uzi-data`）、评分层
（`uzi-pipeline`）、评委层（`uzi-investors`）与报告层（`uzi-report`）；
差别只在**你读哪份工作流**、以及用哪个入口命令。

## 选择最窄匹配的工作流

| 场景 | 读 | 入口 |
|---|---|---|
| 个股研究、估值、IC 备忘录、首次覆盖、催化剂、财报解读、HTML 报告 | [`skills/deep-analysis/SKILL.md`](skills/deep-analysis/SKILL.md) | `uzi <ticker> --stage1` / `--stage2` |
| 评审团、"哪些大佬会买"、只要投票、persona 评审 | [`skills/investor-panel/SKILL.md`](skills/investor-panel/SKILL.md) | 同上（Task 3 产物 `panel.json`） |
| 游资、龙虎榜、席位识别、A 股短线资金分析 | [`skills/lhb-analyzer/SKILL.md`](skills/lhb-analyzer/SKILL.md) | 同上（`16_lhb` 维度） |
| 杀猪盘、"老师/群/朋友推荐"、安全性排查 | [`skills/trap-detector/SKILL.md`](skills/trap-detector/SKILL.md) | 同上（`18_trap` 维度） |

> 四个 skill 由同一条流水线产出：跑一次 `--stage1` 就同时拿到 22 维数据、评分、评委
> `panel.json`、LHB 席位匹配与 `18_trap` 杀猪盘扫描。**不需要为每个 skill 各跑一遍。**

## ⚡ 命令速查（`commands/`）

`commands/*.md` 是逐个方法的操作卡片。**每个方法都有可直接执行的 CLI 入口**：

```bash
uzi <ticker> --stage1              # 先建缓存（--method 全部依赖它）
uzi <ticker> --method dcf          # 单方法结果，stdout 纯 JSON
uzi --portfolio p.csv --method rebalance
```

| 命令 | 用途 | 入口 |
|---|---|---|
| [`analyze-stock`](commands/analyze-stock.md) | 全流程深度分析（主入口） | `--stage1` → 写 `agent_analysis.json` → `--stage2` |
| [`quick-scan`](commands/quick-scan.md) | 快速扫描 | `--depth lite --no-browser` |
| [`panel-only`](commands/panel-only.md) | 只要评审团 | `--stage1` + 读 `panel.json` |
| [`scan-trap`](commands/scan-trap.md) | 杀猪盘排查 | `--stage1` + 读 `18_trap` |
| [`screen`](commands/screen.md) | 每日全市场筛选 | `uzi --screen daily` |
| [`segmental-model`](commands/segmental-model.md) | 分业务收入建模 | `--segmental discover` / `validate` |
| [`dcf`](commands/dcf.md) · [`comps`](commands/comps.md) · [`lbo`](commands/lbo.md) | 估值建模 | `--method dcf` / `comps` / `lbo` |
| [`ic-memo`](commands/ic-memo.md) · [`dd`](commands/dd.md) | 投委会 / 尽调 | `--method ic-memo` / `dd` |
| [`initiate`](commands/initiate.md) · [`catalysts`](commands/catalysts.md) · [`thesis`](commands/thesis.md) | 研究产物 | `--method …` |
| [`earnings`](commands/earnings.md) · [`earnings-preview`](commands/earnings-preview.md) | 财报解读 / 前瞻 | `--method …` |
| [`model-update`](commands/model-update.md) · [`ai-readiness`](commands/ai-readiness.md) | 模型更新 / AI 就绪度 | `--method …` |
| [`rebalance`](commands/rebalance.md) · [`returns`](commands/returns.md) | 组合再平衡 / 收益归因 | `uzi --portfolio <csv> --method …` |

`--method` 的完整 NAME 清单见 [`commands/`](commands/) 与
[`skills/deep-analysis/SKILL.md`](skills/deep-analysis/SKILL.md)。

## 默认执行

```bash
cargo build --release -p uzi-cli          # 产物: target/release/uzi
```

```bash
uzi 600519.SH                             # 一把跑完（快速模式，评委为规则引擎输出）
uzi 贵州茅台 --no-browser                  # 中文名会自动解析
uzi 600519.SH --stage1                    # 停下等 agent 介入（deep 档必须走这条）
uzi 600519.SH --stage2                    # 合并 agent_analysis.json 后出报告
uzi 600519.SH --remote                    # 生成公网链接，手机可看
uzi --versus 600519.SH 000858.SZ          # 横向对比
uzi --portfolio holdings.csv              # 组合批量分析
uzi --screen daily                        # A+港股每日筛选
```

排障 / 维护：

```bash
uzi --preview          # 内置 mock 数据离线出报告（验证模板，不联网）
uzi --browser-check    # 探测 CDP 浏览器兜底依赖
uzi --prewarm          # 预热跨股公共缓存
uzi --xueqiu-status    # 雪球登录状态
```

## 目录结构

```
SKILL.md                          # 本文件（根索引）
skills/
  deep-analysis/SKILL.md          # 深度分析工作流（核心）
  deep-analysis/personas/*.yaml   # 51 份 persona 档案（12 flagship + 39 stub）
  investor-panel/SKILL.md
  lhb-analyzer/SKILL.md
  trap-detector/SKILL.md
assets/                           # 报告模板 / 头像 / 免责声明 / 数据契约
crates/                           # uzi-core · uzi-data · uzi-features · uzi-investors
                                  # uzi-models · uzi-pipeline · uzi-report · uzi-review
                                  # uzi-screen · uzi-cli
tools/golden/                     # 对照上游 Python 的差分测试与 golden 产物
```

`personas/` 是**功能性资产**，不是文档：`uzi_investors::persona_yaml::personas_dir()`
默认解析 `skills/deep-analysis/personas`（相对 cwd），因此**从仓库根目录运行 `uzi`**
才能让 flagship persona 生效；否则 `export UZI_PERSONAS_DIR=<repo>/skills/deep-analysis/personas`。

## Agent 规则

1. 把脚本当数据与打分工具，**不要**当作最终分析结论。
2. **不要编造数字**。用脚本产物、缓存 JSON，或当前公开可检索的证据。
3. 严肃的深度分析请求：必须走完 `skills/deep-analysis/SKILL.md` 描述的 agent review 闭环
   （写 `agent_analysis.json`）再出报告。
4. **`--depth deep` 不是快速模式** —— 必须由你介入 role-play 并写 `agent_analysis.json`；
   只有 lite/medium 才适合 `uzi <ticker>` 一把梭。
5. 游资分析：先做席位匹配与 `is_in_range()` 再给短线判断。
6. 杀猪盘检测：8 个信号全部扫描，风险非平凡时必须给具体证据。
7. 报告模板 / UI 改动：同步更新测试与 golden 产物。

## 环境变量速查

| 变量 | 作用 |
|---|---|
| `UZI_DEPTH` | `lite` / `medium` / `deep` |
| `UZI_SCHOOL` | 锁定流派 A–I |
| `UZI_PERSONAS_DIR` | 覆盖 persona 目录 |
| `UZI_CACHE_ROOT` | 覆盖 `.cache` |
| `UZI_REPORTS_DIR` | 覆盖报告输出目录 |
| `UZI_ASSETS_DIR` | 覆盖 `assets/` |
| `UZI_SKIP_REVIEW` | `1` = 跳过自查门控（**仅调试**） |
| `UZI_PLAYWRIGHT_ENABLE` / `UZI_PLAYWRIGHT_FORCE` | 浏览器兜底开关 |
| `UZI_XQ_LOGIN` | `1` = 启用雪球登录态抓取 |
| `MX_APIKEY` | 设置后 MX 妙想 API 参与解析与补齐 |

完整清单与数据契约见 [`skills/deep-analysis/SKILL.md`](skills/deep-analysis/SKILL.md)。
