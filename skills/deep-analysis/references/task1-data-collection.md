# Task 1 · 数据采集（多 Agent 并行版）

把 22 维（19 采集维 + 3 机构建模维）所需的全部原始数据采集落地为 `.cache/{ticker}/raw_data.json`。

> **v2.0 关键变化**：原来的 19 维 (0_basic → 18_trap, 外加 19_contests) 全部保留；Task 1 结束后，`uzi <ticker>` 会自动触发 **Task 1.5 · 机构级建模**（入口 `uzi_cli::stages`），向 raw_data.json 注入 3 个新维度：
> - `20_valuation_models` — DCF / Comps / 3-Statement / LBO
> - `21_research_workflow` — Initiating Coverage / Earnings / Catalyst Calendar / Thesis / Morning Note / Idea Screens / Sector Overview
> - `22_deep_methods` — IC Memo / Unit Economics / VCP / DD Checklist / Porter 5 Forces + BCG / Portfolio Rebalance
>
> 这 3 个维度是**纯计算**的，不用再发 HTTP 请求。详细方法论见 `task1.5-institutional-modeling.md` 和 `fin-methods/README.md`。


## 🚀 并行执行策略（核心优化）

**禁止串行跑 19 个 fetcher**。正确做法：

### Step 0 · 强制刷新实时数据
```bash
export STOCK_NO_CACHE=1   # bash
set STOCK_NO_CACHE=1      # cmd
$env:STOCK_NO_CACHE='1'   # powershell
```
确保第一次拉的 basic 是真实时数据。后续 fetcher 会按各自的 TTL 自动判断是否复用缓存（TTL 见源码 `uzi_core::cache`（运行时不读；缓存 TTL 由二进制内建））。

### Step A · 先跑一个基础 fetcher（必须串行）
```bash
uzi {ticker_or_name} --stage1
```
Stage 1 内部**先落**基础快照 fetcher `uzi_data::fetch::basic`，拿到 `name / code / industry / market_cap / price`，再并行展开其余 fetcher。后续 4 个 agent 都依赖这些字段。

> 若只需单独取基础快照、不展开其余维度，直接调用 `uzi_data::fetch::basic`（Stage 1 内部走的同一条链路）。

**⚠️ 数据时效检查**：拿到 basic 输出后，先告诉用户：
```
📊 数据快照: 2026-04-14 14:32:18  ·  📈 市场状态: 交易中
```
如果当前在交易日且 basic 返回的时间戳与现在差 > 5 分钟，**立刻用 `STOCK_NO_CACHE=1 uzi {ticker} --stage1` 重跑**并通知用户"已强制刷新"。

### Step B · 并行 spawn 4 个子 agent

**在同一个 message 里发出 4 个 Agent 工具调用**（并行执行），每个 sub-agent 负责一个 batch：

```
Agent 1 · 财报派 (Fundamentals)
  模块: uzi_data::fetch::financials, uzi_data::fetch::valuation, uzi_data::fetch::research, uzi_data::fetch::moat
  维度: 1, 6, 10, 14
  prompt: "你负责采集 {ticker} 的基本面数据。依次调用 4 个 fetcher 模块，把输出汇总成一个 JSON 返回。模块失败时降级到 web search 这只票的财报指标 ROE/净利率/PE/PB。"

Agent 2 · 行情派 (Market Action)
  模块: uzi_data::fetch::kline, uzi_data::fetch::capital_flow, uzi_data::fetch::lhb
  维度: 2, 12, 16
  prompt: "你负责采集 {ticker} 的市场行为数据。跑 K线 + 资金面 + 龙虎榜三个 fetcher，识别游资席位，返回 JSON。"

Agent 3 · 行业派 (Industry & Macro)
  模块: uzi_data::fetch::peers, uzi_data::fetch::chain, uzi_data::fetch::industry, uzi_data::fetch::materials, uzi_data::fetch::futures, uzi_data::fetch::macro_, uzi_data::fetch::policy
  维度: 3, 4, 5, 7, 8, 9, 13
  prompt: "你负责采集 {ticker} 的行业链条与宏观环境。跑 7 个 fetcher + web search 补全宏观/政策/原材料三个定性维度，返回 JSON。"

Agent 4 · 情绪派 (Sentiment & Safety)
  模块: uzi_data::fetch::governance, uzi_data::fetch::events, uzi_data::fetch::sentiment, uzi_data::fetch::trap_signals, uzi_data::fetch::contests
  维度: 11, 15, 17, 18, 19
  prompt: "你负责采集 {ticker} 的治理/事件/舆情/杀猪盘/实盘比赛数据。维度 18 是必须的安全维度，要 web search 8 个杀猪盘信号。维度 19 走真实抓取链路（雪球 cube leaderboard + 淘股吧 + 同花顺模拟盘）。"
```

### Step C · Merge & 进度报告

四个 agent 全部返回后，Claude 主线程：
1. 用 `uzi_data::collect` 把 4 个部分 merge 到统一的 `raw_data.json`
2. 输出**进度条**给用户：

```
✅ Task 1/5 · 数据采集完成
[████████████████████] 100% · 19/22 维（19 采集维 + 3 机构建模维） · 用时 1m 23s
   ├─ Agent 1 (财报派)   ✓ 4/4 维
   ├─ Agent 2 (行情派)   ✓ 3/3 维
   ├─ Agent 3 (行业派)   ✓ 7/7 维
   └─ Agent 4 (情绪派)   ✓ 5/5 维
```

进度条每运行完一个 task 都要输出一次（5 次总）。

## 进度条格式规范

每完成一个 Task 必须打印：
```
[████████░░░░░░░░░░░░] {pct}% · Task {n}/5 · {task_name}
```

进度计算（每 task 占 20%）：
- Task 1 完成 → 20%
- Task 2 完成 → 40%
- Task 3 完成 → 60%
- Task 4 完成 → 80%
- Task 5 完成 → 100%

格式细节：
- 用 `█` (U+2588) 实心和 `░` (U+2591) 空心，**总宽度固定 20 字符**
- 后面跟百分比和当前 task 名
- 全部输出在一行（不要被 Markdown 折断）

## 通用执行原则

1. **优先调用 fetcher 模块**（`uzi_data::fetch::*`），失败 ≥2 次再降级到 web search
2. **每条数据必须带 source 字段**：`"em_push2his:kline"` / `"web:eastmoney.com/article/123"`
3. **降级标记**：模块失败用了 web search 的，在 raw_data.json 写 `"fallback": true`
4. **缓存命中**：`uzi_data::sources` 与 `uzi_core::cache` 已内置分级 JSON 缓存，重复跑同一只票直接返回缓存

> **📡 每个字段的稳定数据源清单**: 见 `references/data-sources.md` — 所有字段对应的 HTTP 端点 + fallback 链都在那里，是 single source of truth。

## 22 维（19 采集维 + 3 机构建模维） fetcher 映射

| 维度 | fetcher 模块 | 备用 web search 关键词 |
|---|---|---|
| 0 基础信息 | `uzi_data::fetch::basic {ticker}` | `{name} 股票代码 行业 市值` |
| 1 财报 | `uzi_data::fetch::financials {ticker}` | `{name} 营收 净利润 ROE 现金流` |
| 2 K线 | `uzi_data::fetch::kline {ticker}` | `{name} K线 均线 MACD` |
| 3 宏观 | `uzi_data::fetch::macro_ {industry}` | `2026 货币政策 {industry} 宏观` |
| 4 同行 | `uzi_data::fetch::peers {ticker}` | `{name} 竞争对手 同行 对比` |
| 5 上下游 | `uzi_data::fetch::chain {ticker}` | `{name} 上游 下游 客户 供应商` |
| 6 研报 | `uzi_data::fetch::research {ticker}` | `{name} 研报 评级 目标价` |
| 7 行业景气 | `uzi_data::fetch::industry {industry}` | `{industry} 景气度 增速 TAM` |
| 8 原材料 | `uzi_data::fetch::materials {ticker}` | `{name} 原材料 成本 价格` |
| 9 期货 | `uzi_data::fetch::futures {industry}` | `{industry} 关联期货 持仓` |
| 10 估值 | `uzi_data::fetch::valuation {ticker}` | `{name} PE PB DCF 历史分位` |
| 11 治理 | `uzi_data::fetch::governance {ticker}` | `{name} 实控人 质押 增减持 关联交易` |
| 12 资金面 | `uzi_data::fetch::capital_flow {ticker}` | `{name} 北向 融资融券 股东户数` |
| 13 政策 | `uzi_data::fetch::policy {industry}` | `{industry} 政策 监管 补贴` |
| 14 专利 | `uzi_data::fetch::moat {ticker}` | `{name} 专利 护城河 研发占比` |
| 15 事件 | `uzi_data::fetch::events {ticker}` | `{name} 公告 催化剂 新订单` |
| 16 龙虎榜 | `uzi_data::fetch::lhb {ticker}` | `{name} 龙虎榜 席位` |
| 17 舆情 | `uzi_data::fetch::sentiment {ticker}` | `{name} 雪球 股吧 大V` |
| 18 杀猪盘 | `uzi_data::fetch::trap_signals {ticker}` | `{name} 推荐 群 直播 预警` |
| 19 实盘赛 | `uzi_data::fetch::contests {ticker}` | `{name} 实盘 持仓 淘股吧` |

## 🌟 Bonus 数据 (v0.6+)

除了 22 维（19 采集维 + 3 机构建模维），还有 3 个额外 fetcher / computer 必须在 Task 1 后期执行（先拉 basic → 再 spawn 以下 3 个并行任务）：

| 额外数据 | fetcher 模块 | 说明 |
|---|---|---|
| **基金经理抄作业** | `uzi_data::fetch::fund_holders {ticker}` | 持仓基金 + 每位经理 5 年业绩（收益/回撤/夏普/排名） |
| **相似股推荐** | `uzi_data::fetch::similar_stocks {ticker}` | 同行业 + 股价相关性 top 4 |
| **情景模拟 + 离场触发器** | `uzi_features::friendly {ticker}` | 基于已采集数据自动生成（不需要外部 API）|

**执行时机**: 这 3 个放在 Task 1 最后一步（所有 fetcher 都完成后）跑，因为它们依赖已经写入 `raw_data.json` 的其他字段。

**产出位置**:
- `uzi_data::fetch::fund_holders` → `raw_data.json#/fund_managers`
- `uzi_data::fetch::similar_stocks` → `raw_data.json#/similar_stocks`
- `uzi_features::friendly` → 在 Task 4 时注入 `synthesis.json#/friendly`

## raw_data.json 结构

```json
{
  "ticker": "002273.SZ",
  "name": "水晶光电",
  "market": "A",
  "fetched_at": "2026-04-14T12:34:56+08:00",
  "dimensions": {
    "0_basic": { "data": {...}, "source": "em_push2:stock_individual_info", "fallback": false },
    "1_financials": { ... },
    "2_kline": { ... },
    "...": "..."
  }
}
```

## 完成检查

- [ ] 20 个 dimension key 全部存在（0-19）
- [ ] 每个 key 都有 `data`、`source`、`fallback` 三字段
- [ ] `fetched_at` 是 ISO 8601 with timezone
- [ ] 文件落地到 `.cache/{ticker}/raw_data.json`

完成后向用户汇报：`Task 1 ✓ 采集了 X 条数据，其中 Y 条降级到 web search`。
