# 📡 DATA_SOURCES · 每个字段的稳定来源清单

> **核心规则**：报告里看到的每一块数据，都必须在本文件里找到对应的**稳定来源 + 三级 fallback**。
> 如果某字段找不到稳定源，它就不应该出现在报告里——禁止编排器瞎编。
>
> **优先级规则**：每条数据按 `A → B → C → D` 顺序尝试：
> - **A**: 直连 HTTP 主源（EastMoney push2 / push2his / datacenter、Tencent qt、Sina hq、Yahoo chart v8）
> - **B**: 备用 HTTP 源 / 官方披露站（cninfo、交易所、AASTOCKS）
> - **C**: web search（DuckDuckGo）+ 解析（兜底）
> - **D**: 标记"数据缺失"，卡片不渲染该字段
>
> 上游用 AkShare 的 Python 封装直抓；本项目**没有 Python 依赖**，全部走下方登记的 HTTP 端点，
> 由 `uzi_data::registry` 的 `health` 标注（`known_good` / `flaky` / `blocked_often` / `needs_browser`）
> 决定尝试顺序，由 `uzi_data::sources`（配合 `uzi_core::cache`）承接分级缓存 + retry。
>
> **降级标记**：某字段一旦走 C 层，必须在 raw_data.json 里写 `"fallback": true`；`source` 登记实际命中的源 ID
> （如 `em_push2` / `em_data` / `push2his` / `tencent_qt` / `sina_quote` / `yahoo_chart_v8` / `web:...`）。

---

## 0 · 股票基础信息

| 字段 | A 主源 | B 备源 | C 兜底 |
|---|---|---|---|
| `name` 股票简称 | 东财 push2 单股 `push2.eastmoney.com/api/qt/stock/get`（`em_push2`，`blocked_often`） | push2 全表 `push2.eastmoney.com/api/qt/clist/get` 按代码过滤 | web: "{code} 股票简称" |
| `industry` 行业 | 东财 push2 F10 / `data.eastmoney.com` 板块成分反查（`em_data`） | `uzi_data::sources` 内置行业映射 | 同花顺 F10（CDP 浏览器） |
| `market_cap` 市值 | push2 全表 `总市值` | push2 单股 `总市值` | 东财 datacenter JSON |
| `price` 最新价 | push2 全表 `最新价` (TTL 60s) | 东财 `push2.eastmoney.com/api/qt/stock/get` | 腾讯 `qt.gtimg.cn/q=...`（`tencent_qt`，known_good） |
| `change_pct` 涨跌幅 | push2 全表 `涨跌幅` (TTL 60s) | 同上 | 同上 |
| `pe_ttm` | push2 全表 `市盈率-动态` | 乐咕乐股 PE 历史末行（`legulegu`，`needs_browser`） | 新浪 `money.finance.sina.com.cn` |
| `pb` | push2 全表 `市净率` | 乐咕乐股 PB 历史 | 同上 |
| `one_liner` 一句话定位 | web search "{name} 主营业务" | 从东财 F10 主营构成（`emweb.securities.eastmoney.com/PC_HSF10/BusinessAnalysis`）生成 | 从行业 + 市值 生成 |

**港股**: 东财港股 spot（push2 clist `m:116` 前缀）+ 腾讯 `qt.gtimg.cn/q=hk00700`（`tencent_hk_quote`）
**美股**: Yahoo chart v8（`query1.finance.yahoo.com/v8/finance/chart/{sym}`）的 `meta` 段

基础字段的串联与移植出处（源码，运行时不读）：`uzi_data::fetch::basic`（push2 → 腾讯 qt → 新浪 hq → 内置行业映射）。

---

## C · 加密货币（`market = "C"`）

**入口**：`uzi BTC-USD` / `uzi BTC` / `uzi BTCUSDT` / `uzi SOL-USD`。代码规范化为
`BASE-QUOTE`（如 `BTC-USD`），`currency` = 计价币，`exchange` = `CRYPTO`。
与 A/H/U 不同，加密货币**没有财报**：`1_financials` 变成代币经济、`10_valuation` 变成
NVT/换手、`16_lhb` / `19_contests` 标记为不适用（权重 0）。逐维来源：

| Dim | 主源 | 备源 / 兜底 |
|---|---|---|
| `0_basic` 行情·市值·供应 | CoinGecko `/coins/markets?ids={id}`（`coingecko_markets`） | OKX `/api/v5/market/ticker?instId={symbol}-USDT`；未知币种先走 CoinGecko `/search` 解析 id |
| `1_financials` 代币经济 | CoinGecko `/coins/{id}` → `market_data`（FDV / 流通量 / 总量 / 硬顶） | 由 `market_cap` + `circulating_supply` 自算流通率与 FDV/市值 |
| `2_kline` OHLCV + 指标 | OKX `/api/v5/market/candles?instId={symbol}-USDT&bar=1D`（`okx_spot_tickers` 同源） | Binance `/api/v3/klines` → CoinGecko `/coins/{id}/market_chart`（无 OHLC，降级为收盘价） |
| `3_macro` 加密宏观 | CoinGecko `/global`（总市值 / 24h 变化 / BTC·ETH 占比） | alternative.me `/fng/`（恐慌贪婪指数） |
| `4_peers` 市值同侪 | CoinGecko `/coins/markets?order=market_cap_desc&per_page=15` | — |
| `5_chain` 生态/业务构成 | CoinGecko `/coins/{id}` → `categories` + `description` + `links` | — |
| `6_research` 开发/社区 | CoinGecko `/coins/{id}` → `developer_data` + `community_data` | — |
| `7_industry` 赛道地位 | CoinGecko `/coins/categories` + 市值占比自算 | — |
| `8_materials` 生产成本 | CoinGecko `/coins/{id}` → `hashing_algorithm`（PoW 才有意义） | — |
| `9_futures` 合约费率 | OKX `/api/v5/public/funding-rate` + `/public/open-interest`（`{symbol}-USDT-SWAP`） | — |
| `10_valuation` 估值 | 自算：NVT = 市值 ÷ 24h 成交额、日换手、区间位置、距 ATH 回撤 | CoinGecko `ath_change_percentage` |
| `11_governance` 治理/解锁 | CoinGecko 总量 vs 流通量 → 未流通比例 | CoinGecko 官方链接 |
| `12_capital_flow` 资金面 | 自算 7 日均量变化 + 稳定币总市值（`ids=tether,usd-coin,dai,first-digital-usd`） | — |
| `13_policy` 监管 | 金十 / 同花顺快讯（`uzi_data::news`）按加密 + 监管关键词过滤 | — |
| `14_moat` 护城河 | 自算：市值份额 + 开发者提交/Stars + 社区粉丝 | CoinGecko `developer_data` / `community_data` |
| `15_events` 事件 | 金十 / 同花顺快讯按币名 + 加密关键词过滤 | — |
| `16_lhb` | **不适用**（加密无龙虎榜/席位） | — |
| `17_sentiment` 情绪 | alternative.me `/fng/`（30 日历史）+ CoinGecko `/search/trending` | CoinGecko `sentiment_votes_up_percentage` |
| `18_trap` 风险扫描 | 自算：24h/7d 涨幅、换手率、成交额深度、距 ATH 回撤、年化波动 | — |
| `19_contests` | **不适用**（无 A 股实盘赛） | — |
| `similar_stocks` | CoinGecko `/coins/markets?category={slug}`（同赛道前 5） | 市值前 15 兜底 |

**估值模型（dim 20–22）**：不做 DCF / LBO / 三表（代币没有现金流与杠杆收购口径），
改为 **NVT 网络价值折现**——`公允价值 = 24h 成交额 × 目标 NVT ÷ 流通量`，
目标 NVT 取 40×（L1/L2 公链，经验区间 20–60×）/ 25×（其他）。稳定币与封装资产
不适用，直接标 `不适用` 而不是编造目标价。首次覆盖评级 / 目标价 / IC Memo / Porter+BCG
均由该模型派生。

**缓存与降级**：所有请求走 `uzi_core::cache` 分级缓存（行情 5min，详情/全局 2h–24h）。
CoinGecko 速率受限时先降级到 OKX（行情/K线/费率），再降级到 CoinGecko `/market_chart`。
失败维度按 `uzi-review` 的加密检查表（`CRYPTO_CHECKS`）生成恢复任务，提示的源是
CoinGecko / OKX / alternative.me，不会指向雪球或东财 F10。

---

## 1 · 财报 (Dim 1)

viz 需要的字段 → 来源：

| viz 字段 | A 主源 | 备注 |
|---|---|---|
| `roe_history` 5年ROE | 东财 F10 主要财务指标 `RPT_F10_FINANCE_MAINFINADATA`（`datacenter.eastmoney.com`）→ `加权净资产收益率(%)` | 取最近 5 年年末值 |
| `revenue_history` 5年营收 | 同上 → `营业总收入` | 单位换算到亿 |
| `net_profit_history` 5年净利 | 同上 → `归属母公司所有者的净利润` | 同上 |
| `financial_years` 年度标签 | 同上 → `报告期` | 格式化 "2020"/"25Q1" |
| `dividend_years` 分红年度 | 东财 F10 分红送配（`data.eastmoney.com` 数据中心）→ `公告日期`；无接口数据时降级 web search | |
| `dividend_amounts` 分红金额 | 同上 → `派息` (元/10股) | |
| `dividend_yields` 股息率 | 自算: `dividend / price_at_year_end * 100` | 基于 kline 收盘价 |
| `financial_health.current_ratio` | 东财 F10 `流动比率` | |
| `financial_health.debt_ratio` | 东财 F10 `资产负债率(%)` | |
| `financial_health.fcf_margin` | 自算: `经营现金流 / 净利润 * 100`（现金流字段取自 F10 现金流表；接口缺失则该项判空） | |
| `financial_health.roic` | 东财 F10 `总资产净利率(%)` | 近似 |

移植出处（源码，运行时不读）：`uzi_data::fetch::financials`（主数据走 `uzi_data::sources` 的 `fetch_financials`）。
**港股 fallback**: 东财港股 F10 摘要
**美股 fallback**: Yahoo `ws/fundamentals-timeseries/v1/finance/timeseries/`（`query2.finance.yahoo.com`）

---

## 2 · K 线 (Dim 2)

| viz 字段 | A 主源 | B 直连 HTTP | C |
|---|---|---|---|
| `candles_60d` OHLC 60日 | 东财 `push2his.eastmoney.com/api/qt/stock/kline/get` secid=1/0.{code} klt=101 fqt=1 取最后 60 行 | 新浪 `money.finance.sina.com.cn/quotes_service/api/json_v2.php/CN_MarketData.getKLineData` | 腾讯 `web.ifzq.gtimg.cn/appstock/app/fqkline/get` |
| `ma20_60d` MA20 序列 | 自算: rolling mean of 20 日收盘 | 同上 | 同上 |
| `ma60_60d` MA60 序列 | 自算: rolling mean of 60 日收盘 | 同上 | 同上 |
| `close_60d` 60日收盘 | 东财 push2his 日线 `收盘` 列 | 同上 | 同上 |
| `stage` Weinstein 阶段 | 自算: 基于价格 vs MA200 + MA200 斜率 | — | — |
| `ma_align` 均线排列 | 自算: MA5>MA10>MA20>MA60 判断 | — | — |
| `macd` | 自算 (ema12, ema26, dif, dea) | — | — |
| `rsi` | 自算 RSI14 | — | — |
| `kline_stats.beta` | 东财 push2his 上证指数日线（secid=1.000001）相关性 计算；取不到降级 web | web | — |
| `kline_stats.volatility` | 自算: std(daily_return) * sqrt(252) | — | — |
| `kline_stats.max_drawdown` | 自算: `(trough - peak) / peak` 近 1 年 | — | — |
| `kline_stats.ytd_return` | 自算: `(last - ytd_open) / ytd_open` | — | — |

**6 路 fallback 链** 移植出处（源码，运行时不读）：`uzi_data::fetch::kline`：
1. 东财 `push2his.eastmoney.com/api/qt/stock/kline/get`
2. 新浪 `money.finance.sina.com.cn`（getKLineData）
3. BaoStock 官方接口 `http://baostock.com/`（无 key）
4. 东财 push2his 直连（同 1，独立解析路径）
5. 新浪 `quotes_service` 直连
6. 腾讯 `web.ifzq.gtimg.cn/appstock/app/fqkline/get`

---

## 3 · 宏观 (Dim 3) · qualitative

Agent web search only，不走行情接口。prompt 模板在 `uzi_data::fetch::macro_`。

---

## 4 · 同行对比 (Dim 4)

| viz 字段 | A 主源 | B 备源 |
|---|---|---|
| `peer_table` 行业前 5 同类 | 东财 `data.eastmoney.com` 板块成分（取 top 5 by 市值） | 申万行业分组（无结构化接口时降级 web search） |
| `peer_comparison` 自己 vs 均值 | peer_table 聚合算均值 | — |

每条 peer 需要 `pe / pb / roe / revenue_growth` → 对每个 peer 再查一次 push2 单股 + 东财 F10 指标。
移植出处（源码，运行时不读）：`uzi_data::fetch::peers`（`push2.eastmoney.com/api/qt/clist/get` 板块成分）。

---

## 5 · 上下游 (Dim 5)

| viz 字段 | A 主源 | B 备源 |
|---|---|---|
| `main_business_breakdown` 主营饼 | 东财 F10 主营构成 `emweb.securities.eastmoney.com/PC_HSF10/BusinessAnalysis` → `分产品/分地区` | 同花顺 F10（CDP 浏览器） |
| `upstream` 上游描述 | 从主营构成 + web search 生成 | — |
| `downstream` 下游描述 | web search "{name} 下游客户 前五大" | — |
| `client_concentration` 客户集中度 | cninfo 年报附注（网页 / PDF 解析）"前五大客户" | web search |
| `supplier_concentration` 供应商集中度 | cninfo 年报附注 "前五大供应商" | web search |

移植出处（源码，运行时不读）：`uzi_data::fetch::chain`。

---

## 6 · 研报 (Dim 6)

| viz 字段 | A 主源 |
|---|---|
| `coverage` 覆盖券商数 | 东财研报接口（`data.eastmoney.com`，`em_data`）count unique orgs |
| `rating` 评级分布 | 同上，聚合 `评级` 字段 |
| `target_avg` / `target_max` / `target_min` | 同上，聚合 `目标价` 字段 |
| `recent_reports` 近 10 研报 | 同上 head(10) |

移植出处（源码，运行时不读）：`uzi_data::fetch::research`（主源 `uzi_data::sources` 的 `fetch_research_reports`）；
cninfo 业绩预测接口为签名 POST、无 GET 等价实现，取不到时按上游降级为空列表。

---

## 7 · 行业景气 (Dim 7)

| viz 字段 | A 主源 |
|---|---|
| `growth` 行业增速 | web search "{industry} 2026 行业增速" |
| `tam` 市场规模 | web search "{industry} TAM 市场规模" |
| `penetration` 渗透率 | web search |
| `lifecycle` 生命周期 | Agent 判断（导入/成长/成熟/衰退）|
| `matched_boards` 概念板块关联 | 东财 push2 概念板块全表按名称包含过滤（取不到降级 web search）|

内置行业锚点 + 可信搜索 + cninfo 行业 PE 的移植出处（源码，运行时不读）：`uzi_data::fetch::industry`。

---

## 8 · 原材料 (Dim 8)

| viz 字段 | A 主源 | B 备源 |
|---|---|---|
| `core_material` 核心材料 | web search "{name} 原材料 采购" + 年报解析 | — |
| `price_history_12m` 价格走势 | 99 期货网 `www.99qh.com` / 生意社 `www.100ppi.com` 现货价 | 新浪期货 K 线 `stock2.finance.sina.com.cn/futures/api/jsonp.php/.../InnerFuturesNewService.getDailyKLine` |
| `cost_share` 成本占比 | 年报附注 | web search |
| `import_dep` 进口依赖 | web search | — |

移植出处（源码，运行时不读）：`uzi_data::fetch::materials`（`futures_main_sina` 同源 + `INDUSTRY_MATERIALS` 映射）。

---

## 9 · 期货关联 (Dim 9) · qualitative only

web search 识别关联合约 → 新浪期货日线（`InnerFuturesNewService.getDailyKLine`）取价。移植出处（源码，运行时不读）：`uzi_data::fetch::futures`。

---

## 10 · 估值 (Dim 10)

| viz 字段 | A 主源 |
|---|---|
| `pe` 当前 PE | 乐咕乐股 `legulegu.com` 估值历史末行（`needs_browser`，取不到降级 push2 `市盈率-动态`）|
| `pe_history` 5 年 PE | 乐咕乐股估值历史 `pe` 列（约 1250 交易日）|
| `pe_quantile` 5 年分位 | 自算 rank percentile |
| `pb` 当前 PB | 乐咕乐股 → `pb` |
| `pb_quantile` | 自算 |
| `industry_pe` 行业均值 | 东财 `data.eastmoney.com` 板块成分 `市盈率-动态` 均值 |
| `dcf.intrinsic_value` | 自算: `simple_dcf(fcf, growth, wacc)` in `uzi_data::fetch::valuation` |
| `dcf_sensitivity.values` | 自算: 5×4 矩阵 (WACC × growth) |
| `dcf_sensitivity.waccs` | [8, 9, 10, 11, 12] 固定 |
| `dcf_sensitivity.growths` | [6, 8, 10, 12] 固定 |

---

## 11 · 治理 (Dim 11)

| viz 字段 | A 主源 |
|---|---|
| `pledge` 实控人质押 | 东财 datacenter 质押比率（整表过滤）；本项目该接口未纳入文档化 helper，取不到时列表为空 |
| `insider` 近 12 月增减持 | 东财 datacenter 高管增减持；同上，缺失即空 |
| `related_tx` 关联交易占比 | cninfo 年报附注（网页 / PDF 解析）|
| `violations` 违规记录 | push2 全表 ST / 风险名单 + web search |
| `equity_incentive` 股权激励 | 东财 datacenter / cninfo |
| `executive_list` 管理层 | cninfo 高管名录 |
| `exec_compensation` 薪酬 | 年报 |

移植出处（源码，运行时不读）：`uzi_data::fetch::governance`。

---

## 12 · 资金面 (Dim 12)

| viz 字段 | A 主源 |
|---|---|
| `northbound_history` 北向 20 日 | 东财 datacenter 北向持股 `RPT_MUTUAL_HOLDSTOCKNDATE_STA`（`em_data`）tail 20 |
| `northbound_20d` 净买入汇总 | 上行 sum |
| `margin_history` 融资余额 | 深交所 / 上交所披露（`www.szse.cn` / `www.sse.com.cn`）按代码过滤 |
| `margin_trend` 趋势描述 | 自算 |
| `holders_history` 股东户数 | 东财 datacenter 股东户数；取不到时降级 web search |
| `holders_trend` | 自算 连升/连降 |
| `main_history` 主力 5 日 | 东财 push2 资金流（按市场）tail 5 |
| `main_5d` 汇总 | 上行 sum |
| `block_trades_recent` 大宗交易 | 东财 datacenter 大宗交易（按起止日过滤）|
| `unlock_schedule` 12 月解禁 | 东财 datacenter 解禁队列 / 新浪解禁（取不到为空）|
| `institutional_history.quarters` 季度 | 基金持仓季报近 8 季聚合（`uzi_data::fetch::fund_holders`）|
| `institutional_history.fund` 公募持仓 | 同上，type 过滤"公募" |
| `institutional_history.qfii` QFII 持仓 | 同上，type 过滤"QFII" |
| `institutional_history.shehui` 社保持仓 | 同上，type 过滤"社保" / 年金 |

移植出处（源码，运行时不读）：`uzi_data::fetch::capital_flow`。

---

## 13 · 政策 (Dim 13) · qualitative only

Agent web search + 政府域原文（`www.gov.cn` / `www.csrc.gov.cn` / `www.miit.gov.cn` / `www.ndrc.gov.cn` / `www.samr.gov.cn`）+ Agent 判断。移植出处（源码，运行时不读）：`uzi_data::fetch::policy`。

---

## 14 · 护城河 (Dim 14)

| viz 字段 | A 主源 |
|---|---|
| `rd_investment` 研发投入 | 东财 F10 利润表研发费用（缺失时用成本项减出）|
| `rd_pct` 研发占比 | 自算 |
| `patent_count` 专利数 | web search "{name} 专利数量" / 国家知识产权局（CDP 浏览器）|
| `intangible` / `switching` / `network` / `scale` | Agent 从业务描述 + 同行对比 评估 (1-10) |

移植出处（源码，运行时不读）：`uzi_data::fetch::moat`（web search 关键词打分）。

---

## 15 · 事件驱动 (Dim 15)

| viz 字段 | A 主源 |
|---|---|
| `event_timeline` 事件时间线 | 东财快讯（`newsapi.eastmoney.com`）+ cninfo 公告 `www.cninfo.com.cn/new/hisAnnouncement/query` + 财联社 / 金十 / 同花顺快讯 合并按日期排序 |
| `recent_news` 近新闻 | 东财快讯 head 10 |
| `catalyst` 催化剂 | Agent 从 timeline 提炼 |
| `earnings_preview` 业绩预告 | 东财 datacenter 业绩预告（按日期过滤）|
| `warnings` 利空 | push2 风险名单 + web search |

移植出处（源码，运行时不读）：`uzi_data::fetch::events`（`cninfo` + `uzi_data::news`）。

---

## 16 · 龙虎榜 (Dim 16)

| viz 字段 | A 主源 |
|---|---|
| `lhb_records` 30 日上榜 | 东财 datacenter 龙虎榜明细（`data.eastmoney.com`，`em_data`）|
| `matched_youzi` 识别游资 | `uzi_investors::seat_db`（`match_seats_in_lhb`）|
| `inst_vs_youzi.inst_net` 机构净买 | 自算，识别"机构专用"席位 |
| `inst_vs_youzi.youzi_net` 游资净买 | 自算 |
| `sector_lhb` 同板块 | 东财龙虎榜统计（近一月）|

移植出处（源码，运行时不读）：`uzi_data::fetch::lhb`。

---

## 17 · 舆情 (Dim 17)

| viz 字段 | A 主源 |
|---|---|
| `xueqiu_heat` 雪球热度 | 东财 hot rank（`uzi_data::hottrend`）；取不到降级 web search |
| `guba_volume` 股吧讨论量 | 抓 `guba.eastmoney.com/list,{code}.html` 帖子数 |
| `big_v_mentions` 大 V 提及 | web search "雪球 {name}" 聚合（`uzi_data::web_search`）|
| `positive_pct` 正面占比 | web search + 关键词情感分析 |
| `thermometer_value` 温度计 | 自算 0-100 归一化 |

移植出处（源码，运行时不读）：`uzi_data::fetch::sentiment`。

---

## 18 · 杀猪盘检测 (Dim 18) · qualitative

8 信号扫描全部靠 web search，详见 `skills/trap-detector/references/eight-signals.md`。移植出处（源码，运行时不读）：`uzi_data::fetch::trap_signals`。

---

## 19 · 实盘比赛持仓 (Dim 19)

| viz 字段 | A 主源 |
|---|---|
| `xq_cubes_list` 雪球组合列表 | `https://xueqiu.com/query/v1/search/cube/stock.json?q={symbol}&count={limit}&page=1`（带 cookie）|
| `high_return_cubes` 高收益数 | 过滤 total_gain > 50 |
| `tgb_list` 淘股吧讨论 | `https://www.taoguba.com.cn/Article/list/all?keyword={code}` HTML 抓取 |
| `ths_list` 同花顺模拟 | `https://moni.10jqka.com.cn/holder/?stock={code}` 抓取 |

移植出处（源码，运行时不读）：`uzi_data::fetch::contests`。

---

## 🌟 NEW · 基金经理抄作业 (Fund Managers)

**这是新增的面板，最需要稳定源**。

| viz 字段 | A 主源 |
|---|---|
| `fund_holders` 持仓基金列表 | 新浪 `vip.stock.finance.sina.com.cn/corp/go.php/vCI_FundStockHolder/stockid/{code}.phtml` 最近 1-2 季所有持仓基金 |
| `fund_code` 基金代码 | 同上返回 |
| `fund_name` 基金名称 | 同上 |
| `position_pct` 占基金比例 | 同上 `占净值比例` |
| `rank_in_fund` 第几大持仓 | 同上 `排名` |
| `holding_quarters` 持有季度数 | 查近 8 季历史持仓判断 |
| `position_trend` 加仓/减仓 | 对比上季 `持股数量` 变化 |
| `manager_name` 基金经理 | 天天基金 `fund.eastmoney.com/pingzhongdata/{fund_code}.js` 或基金经理列表 match |
| `nav_history` 5 年净值 | 天天基金 `pingzhongdata/{fund_code}.js` tail 5Y（备用 蛋卷 `danjuanfunds.com/djapi/fund/{fund_code}`）|
| `return_5y` 5 年累计收益 | 自算: `(nav[-1] - nav[0]) / nav[0] * 100` |
| `annualized_5y` 年化 | 自算: `((1+return_5y/100)^(1/5) - 1) * 100` |
| `max_drawdown` 最大回撤 | 自算: peak-trough on nav |
| `sharpe` 夏普比率 | 自算: mean(daily_ret) / std(daily_ret) * sqrt(252) · 无风险利率按 3% |
| `peer_rank_pct` 同类排名 | 天天基金同类排名接口 + 基金经理列表 |
| `fund_url` 基金详情链接 | `https://fund.eastmoney.com/{fund_code}.html` |

移植出处（源码，运行时不读）：`uzi_data::fetch::fund_holders`。

**Fallback**:
- B: 天天基金 `fundf10.eastmoney.com/FundArchivesDatas.aspx`（持仓明细）+ 基金日度信息 / 评级组合
- C: web search "{基金名称} 5年业绩" + 天天基金网抓取

---

## 🌟 NEW · 相似股推荐 (Similar Stocks)

| viz 字段 | A 主源 | B 备源 |
|---|---|---|
| `similar_stocks` 前 4 只 | 方法: 同行业 + 概念板块交集 + 股价相关性 > 0.8 | — |
| 数据路径 | 东财 push2 板块成分（行业 ∩ 概念）然后对每个候选算 60 日收益率 pearson 相关；行情兜底走雪球 `xueqiu.com/S/SH{code}` / `SZ{code}` | push2 全表相似度 |
| `similarity` 相似度 % | 自算: 相关系数 * 100 |
| `reason` 理由 | Agent 从业务描述 生成 1 句 |

移植出处（源码，运行时不读）：`uzi_data::fetch::similar_stocks`。

---

## 🌟 NEW · 情景模拟 (Scenario Simulator)

| 场景 | 计算方法 |
|---|---|
| `最坏情况 (-35%)` | entry_price × (1 - 2σ) — 2 倍历史年化波动率 |
| `偏差情况 (-15%)` | entry_price × (1 - 1σ) |
| `合理情况 (+12%)` | entry_price × (1 + 预期收益率)，基于研报目标价均值 |
| `乐观情况 (+38%)` | entry_price × (1 + 1σ + 预期收益率) |
| `极致乐观 (+75%)` | entry_price × (1 + 2σ + 预期收益率) |

`probability` 基于正态分布假设（实际是经验值）:
- -2σ: 5%
- -1σ: 25%
- base: 40%
- +1σ: 25%
- +2σ: 5%

移植出处（源码，运行时不读）：`uzi_features::friendly`。

---

## 🌟 NEW · 离场触发条件 (Exit Triggers)

Agent **从 synthesis 自动生成 5 条**，模板：

1. **技术止损**: "股价跌破 {MA60 值} (60 日均线) → 无条件止损"
2. **基本面恶化**: "{关键依赖} 下修 > 10% → 业绩逻辑动摇" (从 raw_data.5_chain 提取大客户)
3. **业绩不达**: "下次业绩预告低于 +{当前增速下限}% → 预期管理失守"
4. **资金撤离**: "{识别到的顶级游资} 席位大额卖出 > 2 亿 → 顶级游资撤离信号"
5. **估值泡沫**: "PE 站上 5 年 {current + 15}% 分位 → 泡沫区获利了结"

---

## 🌐 浏览器兜底（CDP）· 运行时契约

Stage 1 末尾会对**部分维度**跑一次浏览器兜底（用系统已装的 Chromium 系，经 CDP 驱动，
零额外安装；`uzi --browser-check` 确认可用）。

**哪些维度有兜底策略**（其余维度抓不到就只能降级）：

```
4_peers · 8_materials · 15_events · 17_sentiment · 3_macro
7_industry · 14_moat · 13_policy · 18_trap · 19_contests
```

**每个维度需要什么网络能力**（预检判定该能力不通时，该维度直接跳过兜底、不浪费一次抓取）：

| 维度 | 需要 |
|---|---|
| `4_peers` · `8_materials` · `15_events` · `17_sentiment` · `3_macro` · `14_moat` · `13_policy` · `19_contests` | 国内可达 |
| `7_industry` · `18_trap` | 国内可达 **+** 搜索可达 |

**三重门控**（三者全过才真的抓）：

1. **档位**：`lite` 永不启用；`medium` 需 `UZI_PLAYWRIGHT_ENABLE=1`；`deep` 默认启用
2. **质量**：该维度数据为空、被标 `fallback`、或可用公开字段 < 50%（`QUALITY_THRESHOLD`）
3. **网络**：上表的网络能力在预检里是通的

`UZI_PLAYWRIGHT_FORCE=1` 只**跳过第 2 条**（质量门控），对白名单维度强制重跑一次 ——
档位与网络门控仍然生效。

> ⚠️ 这就是"某维度 data 非空却全是 `—`"时不会自动兜底的原因：`—` 也是非空值，质量门控可能
> 判它"够用"。此时由 agent 显式 `UZI_PLAYWRIGHT_FORCE=1 uzi <ticker> --depth deep --stage1`。

兜底结果会合进 `raw_data.json` 的对应维度，并带 `fallback` 标记；抓不到的仍走
`_data_gaps.json`，不编造。

---

## ⚙️ 缓存 TTL 规则（来自 `uzi_core::cache`）

| 数据类型 | TTL | 举例 |
|---|---|---|
| `TTL_REALTIME` = 60s | 实时行情 | 价格、涨跌幅、市值 |
| `TTL_INTRADAY` = 5min | 盘中数据 | K 线、筹码分布、主力资金、雪球热度 |
| `TTL_HOURLY` = 1h | 小时级 | 个股新闻 |
| `TTL_DAILY` = 2h | 日度聚合 | 龙虎榜、北向、融资融券 (覆盖收盘后窗口) |
| `TTL_QUARTERLY` = 24h | 低频 | 财报、研报、历史估值、机构持仓、分红 |
| `TTL_STATIC` = 7d | 几乎不变 | 行业分类、股票简称 |

**强刷**: `STOCK_NO_CACHE=1` 环境变量绕过全部缓存（`uzi_core::cache` 读取）。

---

## 🛡️ 数据质量约定

1. **每条数据必须带 `source` 字段**：告诉前端这条数据是哪个源拉的（用源 ID，如 `em_data` / `tencent_qt`）
2. **`fallback=True` 标记**：用 web search 兜底时必须标记，报告前端显示 "[网络搜索]" 徽章而非 "[官方接口]"
3. **缺失不编造**：找不到数据时字段置 `null` 或缺席，viz 自动跳过渲染该 sub-panel
4. **零外部依赖**：数据源与缓存全部编译进 `uzi` 二进制，没有运行时安装步骤
5. **接口失败重试 3 次后才走 fallback 链**：移植出处（源码，运行时不读）：`uzi_data::sources` / `uzi_data::providers` 的 retry + failover

---

## 🔍 已知问题 / TODO

- [ ] 基金持仓接口返回格式不稳定，不同季度字段名可能变化，需要多版本适配
- [ ] cninfo 年报附注（客户/供应商集中度）没有结构化 API，只能 PDF / HTML 解析 → 暂时依赖 web search
- [ ] 港股/美股的基金持仓数据源比 A 股弱，fund_managers 面板对港美股初期可能为空
- [ ] 雪球 cookie 6 小时过期，`uzi_data::browser::xueqiu` 目前没有自动刷新机制
- [ ] 淘股吧对爬虫有 WAF，高频访问会被封，建议单次查询 + 缓存 24h

---

**维护者**: 任何 fetcher 的字段变动必须同步更新本文件。本文件是 single source of truth。
