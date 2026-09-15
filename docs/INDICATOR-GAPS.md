# 指标缺口清单 · INDICATOR-GAPS

> 对照 `assets/data-contracts.md`（字段契约）与 `skills/deep-analysis/references/data-sources.md`
> （每字段的来源清单），清点 `.cache/<ticker>/raw_data.json` 真实产物里**哪些指标没落地**。
>
> 这份文档是**可勾选的工作清单**，不是结论报告。每勾掉一项，请重跑审计脚本确认。

## 0 · 复现方式

```bash
# 从仓库根目录运行（只读，不写任何缓存）
python3 tools/audit_field_coverage.py                 # 全标的明细 + 结构性恒空清单
python3 tools/audit_field_coverage.py --quiet         # 只看汇总与恒空
python3 tools/audit_field_coverage.py --ticker BTC-USD
python3 tools/audit_field_coverage.py --json /tmp/audit.json
python3 tools/audit_field_coverage.py --strict        # 见下方"退出码"
```

`tools/audit_field_coverage.py` 只读 `.cache/**/raw_data.json`，不联网、不调 `uzi`。

**退出码**：`0` 正常 · `1` `--strict` 命中（存在 100% 空的维度，**或**存在 `error_string` 字段）·
`2` 找不到任何 `raw_data.json`。

### 口径定义

| 术语 | 含义 |
|---|---|
| **叶子字段** | 递归展开到标量或空集合的路径数。列表只取前 3 个元素的结构，长序列不重复计数。 |
| **missing** | 真缺 —— `null` / `""` / `"—"` / `"待补充"` / `"无"` 等占位 |
| **empty_collection** | 空集合 —— `[]` / `{}`。可能是"确实没有"（如无龙虎榜），也可能是没抓到，**需人工判读** |
| **error_string** | 取数失败留下的错误串 —— `"ImportError: akshare not installed"` / `"HTTP 502"` / `"endpoint empty"`。**非空，但不是数据** |
| **结构性恒空** | 凡是有该字段的标的，它都是空的（`empty_in == present_in`） |
| **`_` 诊断字段** | `_` 开头的内部键，**不计入覆盖率的分子分母**，单独列出。它们是定位"某维度为什么恒空"的最快线索 |

> ⚠️ 百分比对嵌套深度敏感：`17_sentiment` 在 BTC 上有 265 个叶子字段（`hot_trend_mentions`
> 嵌套很深），分母被撑大，所以它的百分比低并不代表"这个维度很健康"。看绝对值比看百分比可靠。

### 工具自身的三次修正（别再重犯）

| 版本 | 缺陷 | 后果 |
|---|---|---|
| v1 | 只统计"每个维度的**顶层字段名**" | 把加密空率报成股票的 3.3 倍，实际是 1.6–2.3 倍 |
| v2 | 递归到叶子，但把 `_` 诊断字段**当成数据**统计，且把错误串当成"有值" | `1_financials` 明明整块没取到，却显示 **0.0% 空** |
| v3 | 修好 —— `_` 键排除出覆盖率、错误串单列一类 | 立刻多抓到 `2_kline.chip_distribution` 一处遗漏 |

## 1 · 现状数字

审计时间 2026-09-14（v3 口径，`002273.SZ` 缓存含 §2.1 修复后的新数据）。

| 标的 | 叶子字段 | 空 | 空率 |
|---|---|---|---|
| `002273.SZ`（A 股） | 1226 | 125 | 10.2% |
| `BTC-USD` | 804 | 111 | 13.8% |
| `DOGE-USD` | 737 | 111 | 15.1% |
| `USDT-USD` | 690 | 142 | 20.6% |
| `MOCK.SZ`（mock，不具参考性） | 247 | 5 | 2.0% |

> BTC 叶子字段从 741→804（§3.2 新增加密指标后字段数上升、空率微降）。
> 加密标的的空里有一批是**口径不适用**（`pe/pb/roe/margin` 等，见 §3.1），
> 不是取数失败 —— 百分比的绝对意义有限，看结构性恒空清单判断更可靠。

**结构性恒空：149 个字段，横跨 24 个维度 key。** 加密三币的空率是 A 股样本的 1.4–2.0 倍 ——
差距不是"源抓不到"，而是字段表本身沿用了股票口径。

**`error_string` 目前仅剩 1 处 A 股网络错误，加密 0 处** —— 加密走 CoinGecko 全通；筹码分布不再产生
AkShare 占位错误，网络不可用时只返回空集合。

```
002273.SZ  19_contests.tgb_mentions[].error   = tgb fetch failed: io: invalid peer certificate: ...
```


---

## 2 · P0 · 结构性缺口（换任何标的都缺）

### 2.1 ✅ 已完成 · `1_financials` 现金流 + 分红（akshare 残留）

**这是原 §7 建议顺序的第 1 项，也是收益最大的一项 —— 已修复并验证。**

改动落在 4 个文件（`crates/uzi-data/` 内）：

| 文件 | 改动 |
|---|---|
| `src/em.rs` | 新增 `cash_flow_report()`（EastMoney `PC_HSF10/NewFinanceAnalysis/xjllbAjaxNew`，按 `companyType` 4→3→1→2 探测）· `dividend_history()`（`RPT_SHAREBONUS_DET`）· `CASH_FLOW_COLUMN_MAP` |
| `src/providers/akshare.rs` | 新增 `fetch_cash_flow_a()` / `fetch_dividend_a()` |
| `src/sources.rs` | 新增 `fetch_cash_flow()` / `fetch_dividend()`，均 `TTL_QUARTERLY` 缓存 |
| `src/fetch/financials.rs` | 用真实端点替换两处 `ImportError: akshare not installed` 占位；新增 `dividend_series()` 按年聚合分红并回溯连续年数 |

**验证（`002273.SZ`，`STOCK_NO_CACHE=1` 重跑 stage1）**：

| 字段 | 修复前 | 修复后 |
|---|---|---|
| `dividend_years` | `null` | `["2011" … "2026"]` —— 16 个连续年（2010 确实没分红） |
| `dividend_amounts` | `null` | `[5.0, 2.0, 1.0, 1.5, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 1.0]` |
| `ocf` / `operating_cash_flow` | 缺失 | `"13.5亿"` |
| `ocf_history` | `null` | `[13.47, 17.87, 12.3, 8.42, 7.09]` |
| `ocf_to_net_income_ratio` | `null` | `1.15` |
| `financial_health.fcf_margin` | 缺失 | `115.0` |
| `1_financials` 叶子字段 | 33（含 3 个 `_` 键） | **45，空 0** |

**连带修好的 4 条投资者规则**（`crates/uzi-investors/src/criteria.rs`）：

| 评委 | 规则 | 分数变化 | 证据文本 |
|---|---|---|---|
| `buffett` | `dividend_history` + `fcf_positive` | 50 → **60** | "自由现金流 115% 健康" · "连续 16 年分红" |
| `graham` | `dividend_history` | 18 → **31** | "连续 16 年分红" |
| `burry` | `fcf_real_not_eps` | 66 → **75** | "FCF 健康 115% · 利润真实" |
| `chanos` | `cash_matches_eps` | 23 → **61**（bearish → neutral） | "OCF/净利 1.1 · 比例合理" |

> 修复前这四条全部是**错的**：`dividend_years` 空 → "分红记录仅 0 年"（buffett/graham 误判失败）；
> `known_fcf` 因 `fcf_known=false` 直接 `Err` 跳过（burry 该条根本没评）；
> `ocf_to_net_income_ratio` 空 → "OCF/净利 0.0 · 大幅背离 · 可能账面利润"（chanos 被冤枉）。

#### ⚠️ 2.1.1 必须知道的副作用：DCF 结论被翻转了，而且翻得更错

这不是我引入的 bug，但**是我的改动激活的**，必须显式记录：

`fcf_latest_yi`（`crates/uzi-features/src/stock_features.rs:969-982`）的取值规则是
**"有现金流量表就用 OCF，否则用 净利润 × 0.8"** —— 也就是说，这个仓库把 **OCF 当作 FCF**。

| | base_fcf | intrinsic/share | 现价 | 安全边际 | 结论 |
|---|---|---|---|---|---|
| 修复前（走 proxy 分支） | 9.38 亿 | ¥23.60 | ¥24.67 | −4.3% | ⚪ 基本合理 |
| 修复后（走真实 OCF 分支） | 13.47 亿 | ¥33.89 | ¥24.67 | **+37.4%** | 🟢 深度低估 |
| **真实 FCF（OCF − capex）** | **6.00 亿** | **≈¥15.1**（外推） | ¥24.67 | ≈ **−63%** | 深度**高估** |

- capex = `CONSTRUCT_LONG_ASSET` = `746,686,839.5`（7.47 亿）—— **就在我现在已经抓到的那份
  现金流量表响应里**，只是没被用上。
- `intrinsic / base_fcf = 2.5160`，修复前后两个观测点**完全相同** → 该 DCF 对 base FCF 严格线性，
  所以上面的外推是可靠的（但**我没有实跑**，只是按两个点线性外推）。
- 也就是说：**修复前的旧数字（9.38）反而更接近真实的 6.00；修复后的 13.47 离真相更远。**
  原因是 OCF ≠ FCF，而 `fcf_is_proxy` 被置为 `false`，把这个混同掩盖掉了。

**待决策**（不建议单方面改）：

- [ ] 选项 A：在 `1_financials` 补 `capex_yi`，并把 `fcf_latest_yi` 改成 `ocf − capex`。
      **这是唯一算得上"正确"的做法**，但它会偏离上游 Python 的语义，
      `crates/uzi-features/tests/golden_features.rs` 的逐字节对拍会红。
- [ ] 选项 B：维持现状，把"OCF 当 FCF"写进契约文档并让报告显式标注 `fcf_is_proxy` 语义。
- [ ] 无论选哪个，**至少要让报告不再把 OCF 直呼为"自由现金流"** —— 目前
      `fcf_margin=115.0` 实际是 `OCF/净利`，不是 FCF margin。

### 2.2 `1_financials` 剩下的资产负债表

- [ ] `_balance_sheet_error = "ImportError: akshare not installed"` 仍在（唯一的残余占位）
- [ ] 判定为**暂不处理**：`financial_health` 里的 `current_ratio` / `debt_ratio` / `roic` /
      `net_margin_pct` 已由其他路径（F10 摘要）提供，资产负债表原始三表边际收益低，
      且其端点需要浏览器握手。若将来要做，参照 §2.1 的 `em.rs` 模式加 `zcfzbAjaxNew`。

### 2.3 ✅ 已完成 · `2_kline.chip_distribution`（本地 OHLCV/换手率模型替代 AkShare）

- [x] `crates/uzi-data/src/fetch/kline.rs` 基于 K 线 OHLCV 与换手率递推筹码分布，输出上游兼容的
      获利比例、平均成本、90%/70% 成本区间与集中度字段。
- [x] 计算不再调用已返回 404 的 `push2his.eastmoney.com/api/qt/stock/cyq/get`，避免把网络错误
      或 `ImportError: akshare not installed` 写入产物。
- [x] 新增无效 K 线与正常多日递推回归测试；来源标注为本地确定性模型。

### 2.4 ✅ 已完成 · `6_fund_holders` 被 CLI 的 fetcher 列表漏掉了

- [x] **根因**：`crates/uzi-cli/src/profile.rs` 的 `all_fetchers()` 列表里**根本没有 `6_fund_holders`**，
      导致 `collect` 的 wave 调度直接跳过该维度，`fill_missing_dims` 填充了空壳
- [x] **修复**：把 `6_fund_holders` 加入 `all_fetchers()`，并同步更新 deep profile 的测试断言（20→21）
- [x] **验证**：`002273.SZ` 重新 stage1 后 `fund_managers` = **595 条**，
      `source=akshare:stock_fund_stock_holder + fund_open_fund_info_em(top N only)`，`fallback=false`

> 注意：fetcher 本身（`crates/uzi-data/src/fetch/fund_holders.rs`）一直是对的——
> 新浪持仓页解析、东财基金 stats、蛋卷基金经理名，三段链路全部通畅。
> 这是**调度层遗漏**，不是数据源问题。

### 2.5 `6_research` 研报 A 股 0 覆盖

**已修复（2026-09-15）**：
- [x] A 股 `coverage=0`、`report_count=0` 的根因：`sources.rs::fetch_research_reports`
      的 `cached` 闭包是硬编码 stub `|| Ok(json!([]))`，从未调用任何 HTTP 端点。
      修复：闭包内实际调用 `reportapi.eastmoney.com/report/list`（AkShare
      `stock_research_report_em` 的同一个公开端点），并按上游逻辑将英文 key
      rename 为 `research.rs` 消费的中文字段名（`报告名称`/`机构`/`东财评级`/
      `日期`/`{year}-盈利预测-收益/市盈率`/`报告PDF链接`）。
- [x] 验证：`002273.SZ` 实测返回 100 份研报、21 家券商覆盖，
      `fallback=false`，`coverage="21 家"`；dim 6 三条打分规则恢复可用
- [ ] 加密侧 `community` / `developer` / `rating_distribution` / `target_price_avg` 恒空（3–4/5 标的）—— 加密侧无研报数据源，属口径不适用，降级保留
- [x] 后果消除：dim 6 的三条打分规则（覆盖券商数 / 目标价上行空间 / 买入占比）恢复可用

证据：`002273.SZ` 的 `6_research` 空率从 71.4% 降至 ~0%（除 `upside` 需当前股价计算外，
`coverage` / `report_count` / `rating` / `brokers` / `recent_reports` / `consensus_eps_*` /
`consensus_pe_*` / `target_price_avg` / `target_avg` 全部有值）
测试：`tests/research_fetch.rs`（2 个 `#[ignore]` 网络测试，`--ignored` 运行全绿）

### 2.6 `12_capital_flow` 部分修复 · 5/8 字段已落地

**已修复（2026-09-14）**：
- [x] `holder_count_history` / `holders_trend`（股东户数）→ `RPT_HOLDERNUM_DET`
- [x] `main_fund_flow_20d` / `main_20d` / `main_5d`（主力资金）→ `push2his.eastmoney.com/api/qt/stock/fflow/daykline/get`
- [x] `block_trades_recent`（大宗交易）→ `RPT_BLOCKTRADE_STA`
- [x] `unlock_recent` / `unlock_schedule`（解禁）→ `RPT_LIFT_STAGE`

**仍缺**：
- [ ] `margin_recent` / `margin_trend`（融资余额）—— 个股融资融券端点未找到，市场级 `RPTA_RZRQ_LSHJ` 已确认可用但为市场汇总，非个股明细

**后果变化**：dim 12 三条打分规则中，**股东户数 3 季连降 + 主力 5 日净流入 两条已可用**，融资余额上升仍失效

证据：`002273.SZ` 的 `12_capital_flow` 空率从 20.0% 降至约 **5.2%**（剩余 `margin_*` + 远期解禁的 `B20_ADJCHRATE` / `A20_ADJCHRATE` 为 `null`）

### 2.7 `17_sentiment` 17 个字段结构性恒空

- [ ] 6 平台热榜 `mentions.{weibo,zhihu,baidu,douyin,toutiao,bilibili}` 全空、`hot_rank` 空
- [ ] `platform_snippets.{guba,zhihu,weibo,xiaohongshu,big_v}` 全空
- [ ] `news_multi_source.sources.{jin10,em_stock_ann,ths_news_today}` 空
- [ ] `big_v_mentions` / `positive_pct` 为 `null`（`sentiment_data_available=false`）
- [ ] 现状只剩 `xueqiu_heat=34` 一个有效数字

### 2.8 `18_trap` 11 个信号明细全空（权重 5 的安全维度）

- [ ] `signals_hit_detail` / `snippets.signal_1..8` 全空 —— 只有 `risk_score` 汇总值，**拿不出任何证据**
- [ ] `pump_dump_signals` / `warning_flags` 空（加密侧同样空）
- [ ] 这是报告里唯一"给结论不给证据"的维度，风险最高

**根因分析（2026-09-14 代码审查结论）—— 不是代码缺失，是环境问题：**

- [x] 实现代码**完整**：`crates/uzi-data/src/fetch/trap_signals.rs` 的 8 信号扫描
      （`signals_hit_detail` / `snippets.signal_1..8`）走 `web_search::search()` 全链路通
- [x] `crates/uzi-data/src/web_search.rs` 用 DuckDuckGo HTML 抓取（`ddg_search()`）——
      当前网络环境下返回空结果，与 `3_macro` / `13_policy` / `15_events` 的
      `_autofill_failed = {'reason': 'MX/ddgs 都没有返回内容'}` **同源**
- [x] 证据：`.cache/002273.SZ/raw_data.json` 里 `18_trap.signals_hit="0/8"`、
      `snippets.signal_1..8` 全空，`_data_gaps.json` 也没把它列为 gap
- [ ] **修复方向**：不是改代码，而是换证据源 —— DuckDuckGo 被墙/被限时抓不到。
      备选：换 `web_search::search()` 的默认后端（如 Bing HTML / 本地预置语料），
      或把 `18_trap` 的权重在无证据时降级（当前"给结论不给证据"评分照样计入，不合理）

### 2.9 `10_valuation` 分位与 PEG 缺

- [ ] `pb_quantile`（空于 4/5 标的）、`pe_history`、`industry_pe_fallback_reason`
- [ ] A 股 `pe_quantile` / `industry_pe` 在 `_data_gaps.json` 里长期 `pending`
- [ ] **PEG 从未被计算过** —— dim 10 打分规则要求"必须报告 PE/PB/PEG/历史分位 4 个数字"，
      而 `PEG` 在 `assets/report-template.html` 里只出现 2 次，都在词条提示表里

---

## 3 · P1 · 加密口径错配（股票字段挂在加密上）

`asset_class="crypto"` 标对了，但字段表没换。以下字段在加密标的上恒为 `null`：

### 3.1 股票字段 —— 决策：**保留 `null`，不改成字符串**

- [x] 评估结论（2026-09-14）：把 `null` 改成 `"不适用"` 看似能降空率，但
      渲染层/打分层对 `null` 已有成熟处理（`is_null()` / 数值解析），改成字符串
      会让 `truthy()` 把它当"有数据"（正是 §5.2 contests 错误串被误判为真的同类坑）。
      且本仓库是上游 Python 的**忠实移植**，改字段值 = 主动偏离上游。
- [x] **决定（2026-09-14 已落地）**：保留这些字段为 `null`（语义=该口径不适用于加密资产），
      已把"N/A"这件事写进 `assets/data-contracts.md` §1.1 加密分节，让审计能区分"不适用"与"取数失败"
- [ ] 字段清单：`0_basic.pe_ttm/pb/eps/actual_controller` ·
      `1_financials.roe/roe_history/net_margin/gross_margin/revenue_history/net_profit_history/dividend_years/financial_health.debt_ratio` ·
      `11_governance.pledge/insider_trades_1y/chairman_turnover` ·
      `8_materials.core_material/cost_share/materials_detail/price_history_12m` ·
      `13_policy.anti_trust/monitoring/subsidy/policy_dir`

### 3.2 真实加密指标 —— ✅ 已补 4 处（2026-09-14，零新端点，纯用 CoinGecko 已有字段）

改动文件：`crates/uzi-data/src/crypto.rs`（`dim_basic` / `dim_tokenomics` / `dim_chain` /
`dim_capital_flow` / `dim_valuation`）。用 CoinGecko `/coins/{id}` 与 `/coins/markets`
**已在抓的响应**里的字段，无需新增请求。

| 维度 | 新增字段 | 来源 | 验证（BTC-USD / ETH-USD） |
|---|---|---|---|
| `0_basic` | `atl` · `atl_change_pct` · `ath_date` | markets 行已有 | BTC atl=67.81 · ath_date 2025-10 |
| `1_financials` | `circulating_ratio_pct` · `max_supply_infinite` · `block_time_minutes` | `/coins/{id}` | BTC 100% / false / 10.0 |
| `5_chain` | `total_value_locked` · `mcap_to_tvl_ratio` · `fdv_to_tvl_ratio` · `block_time_minutes` | `/coins/{id}` | block_time 已落地（BTC 10.0 / ETH 0.0） |
| `10_valuation` | `mcap_to_tvl_ratio` · `fdv_to_tvl_ratio` · `roi_1y_pct` | `/coins/{id}`+markets | ETH roi_1y_pct=4109.56 |
| `12_capital_flow` | `market_cap_change_24h` · `market_cap_change_24h_pct` | markets 行 | BTC -97.9 亿 / -0.63% |

- [x] 顺带清理：`capital_flow.rs` 里两条 AkShare 时代死桩（`fetch_holder_counts` /
      `fetch_main_fund_flow`）已被 `sources::` 替换后仍残留，已删除
- [ ] ⚠️ **TVL 缺口**：CoinGecko `/coins/{id}`（不带 `tickers` 参数）对 ETH/BTC 均返回
      `total_value_locked:null` —— 字段已接好，但要真拿到 TVL 需加 `tickers=true` 参数或
      改走 `/global/decentralized_finance_defi`。**这是新增请求**，列为后续 P2
- [ ] **估值补强（P2 候选）**：MVRV / Realized Cap / SOPR / Puell Multiple 需第三方链上
      API（如 Blockchain Center / Glassnode），超出"零依赖"边界，列为 P2
- [ ] **资金面补强（P2 候选）**：交易所净流入、鲸鱼地址净变化、永续持仓量变化需
      交易所/链上 API，超出零依赖边界，列为 P2

---

## 4 · P2 · 定性维度没有证据落点

- [ ] `3_macro` 的 `rate_cycle` / `fx_trend` / `geo_risk` / `commodity` 空，
      `web_search_snippets.*` 6 个子字段全空
- [ ] `13_policy` 的 `policy_dir` / `subsidy` / `monitoring` / `anti_trust` 空，
      `snippets.*` 4 个子字段全空
- [ ] **`raw_data` 里没有任何字段能承载 URL**，但 `HARD-GATE-QUALITATIVE` 要求这 6 维
      每维 `evidence` ≥ 2 条且每条带具体 URL —— 契约与 schema 对不上
- [ ] 唯一 deep 样本 `BTC-USD/agent_analysis.json` 里 `qualitative_deep_dive` **完全不存在**，
      校验器只给 warning 就放过了 —— 考虑把这条升级为 deep 档的 error

证据：`3_macro._autofill_failed` / `13_policy._autofill_failed` / `15_events._autofill_failed`
= `{'reason': 'MX/ddgs 都没有返回内容'}`

---

## 5 · P2 · 内部诊断字段与错误串

### 5.1 `_` 诊断字段泄漏进产物

`002273.SZ` 的 `raw_data.json` 里有 **13 个** `_` 开头字段（加密三币 0 个）。**其中几条直接解释了上面的空缺：**

```
0_basic._em_direct_err            = HTTP 502
0_basic._fallback_snap            = tencent_qt
0_basic._field_baostock_err       = ImportError: baostock not installed
1_financials._balance_sheet_error = ImportError: akshare not installed
3_macro._autofill_failed          = {'reason': 'MX/ddgs 都没有返回内容'}
13_policy._autofill_failed        = {'reason': 'MX/ddgs 都没有返回内容'}
15_events._autofill_failed        = {'reason': 'MX/ddgs 都没有返回内容'}
19_contests._note                 = XueQiu cubes 接口 2026 起需登录 ...
```

- [ ] 零依赖的 Rust 二进制里仍残留上游 Python 的 `akshare` / `baostock` 依赖串
- [x] ✅ **决策落地（2026-09-14）**：**降级进 `_data_gaps.json`**，且放在**审计/可观测层**而非产物层——
      `tools/audit_field_coverage.py` 新增 `--gaps-json` 模式，为每个标的写
      `.cache/<ticker>/_data_gaps.json`（诊断字段 + 错误串 + missing 汇总），供看板/CI 直接消费；
      主产物 `raw_data.json` **保持字节级忠实**（上游同样内嵌这些诊断），不破坏 golden 测试。

### 5.2 错误串泄漏进用户可见报告

- [x] ✅ **已修（2026-09-14）**：`reports/002273.SZ_20260911/full-report.html` 里
      **能直接搜到 `invalid peer certificate`** —— 淘股吧的 TLS 错误串被写进了报告。
      根因：`crates/uzi-report/src/renderer/contests.rs` 拿 `tgb_mentions`（原始数组）直接
      `disp()` 进"淘股吧提及 · N 次"，而 `[{"error":...}]` 是非空数组 → `truthy()` 误判为"有数据"，
      且 `disp(array)` 把整段 JSON 塞进计数字段。数据层其实**已经**提供了过滤后的
      `tgb_mentions_count`（`fetch/contests.rs:234` 用 `.filter(|t| t.get("error").is_none())`）。
      **修复**：渲染器与 KPI 卡改用 `tgb_mentions_count`（过滤计数），错误串不再泄漏、error-only
      时计数正确显示 0。另加 2 个回归测试锁死。
- [x] ✅ 渲染侧同源问题：`contests.rs:33` 的 `truthy` 误判与第 64/70 行 `disp(&tgb)` 塞计数字段 —— 已随上一条一并修复
- [ ] ⚠️ 上游意图对照：本仓库是上游 Python `contests.py` 的**忠实移植**。修复方向是"用数据层已过滤的
      计数渲染"，不改任何 raw_data.json 字段值，符合忠实契约；但若上游确实直接打印数组，需在
      `docs/data-contracts.md` 或移植说明里记一笔差异。
- [x] `2_kline.chip_distribution.error` 已消除：`kline.rs` 不再调用已返回 404 的 `cyq/get`，改为
      基于 OHLCV/换手率的本地确定性模型；网络或换手率数据不足时返回空集合，不写入错误串。

---

## 6 · P3 · 文档与实现不一致 / 测试债

### 6.1 文档

- [ ] `task5-report-assembly.md` 写 "RADAR 22 维雷达图" —— 模板与报告里 `radar` 出现 **0 次**
- [ ] `SKILL.md` 说 "22 维打分"、`task2-dimension-scoring.md` 标题说 "19 维打分"、
      实际 `dimensions.json` 只有 19 个（`0_basic` 不打分，dim 20–22 只在估值建模区）
- [ ] `assets/data-contracts.md` 仍是上游残留：akshare 源名、`.cache/{ticker}/api_cache/` 目录、
      `task4-synthesis.md` 路径、`share-card.png` / `war-report.png` 输出（实际不产出）
- [ ] `data-sources.md` 末尾「已知问题 / TODO」4 条是否仍然成立，逐条核对

### 6.2 测试债（2026-09-14 实测，**均为既存问题，与 §2.1 改动无关**）

- [x] `crates/uzi-report/tests/assemble_e2e.rs` **在任何干净 checkout 上都不可能通过** ——
      它把 `UZI_CACHE_ROOT` 指向临时目录、只拷 `dimensions/panel/synthesis/raw_data` 四个文件，
      却撞上自检门控要求的 `agent_analysis.json`。
      **已修**：测试内设 `UZI_SKIP_REVIEW=1`（`assemble.rs:625` 已支持该开关）。
- [x] `crates/uzi-models/tests/golden_models.rs` 3 个用例恒红：
      `$.data.initiating_coverage.headline.report_date: "2026-09-11" != "2026-09-14"` ——
      夹具把**易变时间戳**烘进了 golden（`research_workflow.rs:196` 用 `clock::now()`）。
      **已修**：给 `clock::now()` 注入 `UZI_TEST_DATE` 环境变量覆盖（仅在测试文件设置），
      不改 golden 夹具、不剔除字段，保留逐字节对拍的严格性。

> 已用 `git stash` 隔离验证：上述两个失败在**未改动代码的基线上同样复现**，确认非本次引入。
> 全量 `cargo test --workspace --no-fail-fast`：**全部通过，0 失败**。

---

## 7 · 建议修复顺序（已更新 2026-09-14）

1. ~~修 `1_financials` 的 akshare 残留~~ ✅ **已完成**（§2.1）
2. **决策 §2.1.1 的 OCF/FCF 语义** —— 这是唯一会让报告**结论级**数字变动的一项，
   但**需用户拍板**（改会偏离上游 Python 语义、golden 对拍会红），当前挂起、不动
3. ~~补 `6_fund_holders`~~ ✅ **已完成**（调度层遗漏，fetcher 本身完好）
4. ~~补 `12_capital_flow`~~ ✅ **大部分已完成**（§2.6）—— 股东户数 / 主力资金 / 大宗交易 / 解禁
   已落地，仅剩**个股融资融券**端点待找（见下方 9）
5. **按 `asset_class` 换掉加密字段表** —— ✅ **已完成**（§3.2 + §3.1）：已补 4 处真实加密
   指标（ATL/ATH 日期、代币经济、链上 block time、资本面市值变化、估值 ROI）。
   股票字段决定保留 `null` 并已写进契约文档（§3.1 + `assets/data-contracts.md` §1.1），
   TVL 需 `tickers=true` 参数列为 P2
6. **`18_trap` 的信号明细** —— 代码完整，已确认是 DuckDuckGo 环境问题（§2.8）；
   换证据后端或降级权重，需先决策
7. ~~清掉 §6.2 的两处测试债~~ ✅ **已完成**
8. ~~**`2_kline.chip_distribution`** —— 已完成本地 OHLCV/换手率模型替代 AkShare 占位~~ ✅
10. ~~**把 `_` 诊断字段收进 `_data_gaps.json`** + 处理 §5.2 的错误串泄漏~~ ✅ **已完成**（2026-09-14）：
    审计工具新增 `--gaps-json` 写 sidecar（§5.1）；渲染器 + KPI 改用 `tgb_mentions_count`，
    错误串不再泄漏进报告，另加 2 个回归测试（§5.2）
11. ~~**`6_research` A 股 0 覆盖**~~ ✅ **已完成**（2026-09-15）：
    根因是 `sources.rs::fetch_research_reports` 的 `cached` 闭包是硬编码 stub
    `|| Ok(json!([]))`，从未调用任何 HTTP 端点。修复为实际调用
    `reportapi.eastmoney.com/report/list`（AkShare `stock_research_report_em`
    同一端点），按上游逻辑 rename 列名。实测 `002273.SZ` 返回 100 份研报、21 家券商，
    `fallback=false`。新增 2 个 `#[ignore]` 网络测试。

每完成一项，跑一次 `python3 tools/audit_field_coverage.py --strict` 验证。
