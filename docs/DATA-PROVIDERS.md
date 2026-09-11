# 数据源 Providers 指南

UZI-Skill 采用多数据源 + 自动 failover 架构。本文档说明本项目的数据层：

- **Provider 框架**（可选 transport 的优先级链与健康度）：`crates/uzi-data/src/providers/`
  （`akshare` / `baostock` / `direct_http` / `efinance` / `tushare`）与 `crates/uzi-data/src/providers/mod.rs`
  的 `registry()`。
- **数据源注册表**（每个 URL、覆盖市场/维度、健康度的纯配置目录）：`crates/uzi-data/src/registry.rs`
  的 `SOURCES`（73 条）。

> **上游 vs 本项目**：上游以 Python 包（akshare / efinance / tushare / baostock）为 transport，
> 需要额外的第三方 Python 依赖与安装步骤。本项目是 Rust 重实现、**零外部依赖**：没有安装步骤、没有
> Python 依赖。库型 provider 无法在 Rust 中存在时，改为直达这些库底层使用的**公开 HTTP 端点**
> （EastMoney push2 / push2his / datacenter、腾讯 qt、新浪 hq），或在 `is_available()` 返回 `false`
> 让链自动跳过——与上游「库未安装时 failover」的行为一致。

---

## 优先级模型

`get_provider_chain(dim, market)`（`providers/mod.rs` 的 `provider_chain`）在内置默认顺序上取
每个 provider 的可用性，过滤出「覆盖该 market 且 `is_available()` 为真」的子链：

```
akshare (HTTP 直连东财，默认)
  ↓ 挂了 / 无数据
efinance (上游的并行冗余；本项目无 Rust 等价物，恒不可用)
  ↓
tushare (需 TUSHARE_TOKEN；本项目无 Python transport，恒不可用)
  ↓
baostock (仅 A 股；本项目无法复刻其私有 socket 协议，恒不可用)
```

`direct_http`（腾讯 qt / 新浪 hq / etnet）**也在 `registry()` 中注册**，但默认顺序里不出现，
只能通过环境变量覆盖显式启用（与上游一致）。

可用 `UZI_PROVIDERS_<DIM>` 覆盖单维度优先级，如：

```bash
# basic 维度强制优先走 direct_http（腾讯/新浪直连）
export UZI_PROVIDERS_BASIC=direct_http,akshare
```

诊断入口默认列出 `kline` / `financials` / `basic` / `lhb` 四个维度的链。

---

## 已实现的 Providers

`registry()` 的注册顺序（上游声明顺序）：`akshare → efinance → tushare → baostock → direct_http`。

| Provider | requires_key | markets | 本项目可用 | transport / 端点 |
|---|---|---|---|---|
| `akshare` | 否 | A, H, U | ✅ | EastMoney push2 / push2his / datacenter（`crate::em`） |
| `efinance` | 否 | A, H, U | ❌ | 无 Rust 等价物（上游为东财/同花顺/新浪聚合爬虫） |
| `tushare` | **是** | A | ❌ | Tushare Pro REST；本项目无 Python transport |
| `baostock` | 否 | A | ❌ | 私有 socket 协议，无可复刻的公开 HTTP 端点 |
| `direct_http` | 否 | A, H, U | ✅ | 腾讯 qt / 新浪 hq / etnet（仅港股的页面级兜底） |

### ✅ `akshare` · 零配置主源

**可用性**：`is_available()` 恒为 `true`。
**实现**：Rust 侧无法 import AkShare，改为调用 AkShare 封装的那几个**公开 HTTP 端点**，解析复用
`crate::em` 与 `data_sources`：

- `fetch_basic_a` → `ak.stock_individual_info_em` 对应的 push2 spot；
- `fetch_financials_a` → `ak.stock_financial_abstract`；
- `fetch_kline_a` → `ak.stock_zh_a_hist`（push2his，`period` 映射 101/102/103，`adjust=qfq` → fqt=1）。

**特色**：覆盖最广（A/港/美股）。
**限制**：端点 `em_push2` 在 2026 常被反爬拦截（`SOURCES` 中 health = `blocked_often`），
大陆 / 境外均可能 Empty reply；失败时链会继续往下走。

### ✅ `direct_http` · 零 key 直连

**可用性**：恒为 `true`（上游要求 `requests` 可导入，本项目用内置 HTTP 客户端，始终在）。
**实现**：`fetch_quote_tencent` / `fetch_quote_sina` / `fetch_quote_etnet`，字段索引逐字对齐上游
（`parts[3]` 为现价、`parts[45]` 为总市值/亿；新浪 A 股读 `fields[3]`、港股读 `fields[6]`）。
`fetch_quote` 内部顺序为 腾讯 → 新浪 → etnet（etnet 仅港股）。

**注意**：它不在默认链里（默认链只列 akshare/efinance/tushare/baostock）；要启用需 `UZI_PROVIDERS_<DIM>`。

### ❌ `efinance` · 上游并行冗余（本项目不可用）

**可用性**：`is_available()` 恒为 `false`，所有方法返回 `ProviderError("efinance 未安装")`。
**原因**：efinance 是 Python 聚合爬虫，没有单一可复刻的公开 HTTP 端点。
A 股 `get_quote_history` 实际解析到 EastMoney push2his——已由 `akshare` provider 覆盖，所以不会丢数据。

### ❌ `tushare` · 官方 API（本项目不可用，除非有 transport）

**可用性**：`is_available()` 恒为 `false`（上游契约是 `_TS_OK and bool(TUSHARE_TOKEN)`；Rust 侧没有
Python 包，故 transport 恒不存在）。`token_present()` 单独检查 `TUSHARE_TOKEN` 是否已设置。
**symbol**：`ts_code()` 仍被移植并供调用方格式化：`600519 → 600519.SH`、`000001 → 000001.SZ`、
`430047 → 430047.SZ`、`832000 → 832000.BJ`。
**说明**：本项目里 tushare 永远不会出现在可用链里；环境变量 `TUSHARE_TOKEN` 只影响
`token_present()`，与 MX 妙想 API 的 `MX_APIKEY` 是同类的 token 机制。
**特色（上游语境）**：字段质量最高、财报 5–10 年三表齐全、龙虎榜/北向/期货等机构级衍生数据。

### ❌ `baostock` · 零配置兜底（本项目不可用）

**可用性**：`is_available()` 恒为 `false`，方法返回 `ProviderError("baostock 未安装")`。
**原因**：BaoStock 走自有（非 HTTP）socket 会话 `login()` / `query_history_k_data_plus()`，
没有文档化 HTTP 端点可复刻。
**symbol**：`bs_code()` 仍被移植：`600519 → sh.600519`、`000001 → sz.000001`。

---

## 数据源注册表（`SOURCES`）

`crates/uzi-data/src/registry.rs` 的 `SOURCES` 是纯配置目录（无 I/O），对齐上游数据源注册表的
声明顺序，共 **73 条**：

- **tier 1**：41 条（HTTP 优先，`http_sources_for` 按 `known_good → flaky → blocked_often → needs_browser` 排序）；
- **tier 2**：11 条（多为需要浏览器会话 / Playwright）；
- **tier 3**：21 条（官方披露 / 交易所 / 政策源）。

每条记录含 `id` / `name_cn` / `base_url` / `markets` / `dims` / `tier` / `access` / `health` / `notes`。
`health` 取值：

| health | 含义 |
|---|---|
| `known_good` | 2026 验证正常 |
| `flaky` | 时好时坏（接口变更 / 403 / 404） |
| `blocked_often` | 常被反爬拦截 |
| `needs_browser` | HTTP 直访被拒，需浏览器会话 |

`access` 取值：`http`（49）/ `ddgs`（11）/ `playwright`（7）/ `akshare`（4）/ `mx_api`（1）/ `yfinance`（1）。

### Tier 1（41 条）

| id | 名称 | base_url | health | notes |
|---|---|---|---|---|
| `em_push2` | 东方财富 push2 | `https://push2.eastmoney.com/api/qt/stock/get` | blocked_often | 2026 常被反爬拦截（大陆 / 境外均可能 Empty reply）；建议走 MX API 或 XueQiu akshare 代抓 |
| `em_quote` | 东方财富 quote 页 | `https://quote.eastmoney.com/` | known_good | push2 挂掉时 quote 子域通常仍可用（2026 验证：200 OK） |
| `em_data` | 东方财富 data 子域 | `https://data.eastmoney.com/` | known_good | 龙虎榜 / 北向 / 融资融券 / 股东户数 / 研报 / 行业板块成分（akshare board_industry_cons_em 走这里） |
| `xq_api` | 雪球 akshare backend | `https://stock.xueqiu.com/` | known_good | akshare.stock_individual_basic_info_xq / stock_individual_spot_xq |
| `tencent_qt` | 腾讯行情 qt | `https://qt.gtimg.cn/` | known_good | realtime quote 兜底源；格式 ~-delimited 字符串 |
| `sina_quote` | 新浪财经行情 | `https://finance.sina.com.cn/` | flaky | hq.sinajs.cn 老接口 2026 返 403；主页 HTML 解析仍可用 |
| `cninfo` | 巨潮资讯 | `http://www.cninfo.com.cn/` | known_good | A 股公告原文的法定披露源；akshare.stock_industry_pe_ratio 也走这里 |
| `hkexnews` | HKEXNews 港交所披露易 | `https://www1.hkexnews.hk/` | known_good | 港股公告的法定披露源 |
| `aastocks` | AASTOCKS 港股 | `https://www.aastocks.com/` | flaky | 港股 PE/PB/industry/南北向核心数据源；HTML regex 抓取 + Playwright 兜底 |
| `cls` | 财联社 7x24 电报 | `https://www.cls.cn/` | known_good | 事件驱动首选；催化剂与突发新闻密度最高 |
| `yicai` | 第一财经 | `https://www.yicai.com/` | known_good | 行业与公司新闻、宏观产业专题；适合 agent 抽取定性评语 |
| `wallstreetcn` | 华尔街见闻 | `https://wallstreetcn.com/` | flaky | 快讯 + 海外联动；/live 端点 2026 返 404，走主页抓最新 |
| `cfi` | 中财网 | `https://quote.cfi.cn/` | known_good | 个股资料 / 公告 / 研报 HTML 兜底 |
| `hexun` | 和讯网 | `https://stock.hexun.com/` | known_good | 研报转载 + 行业点评兜底 |
| `163money` | 网易财经 | `https://money.163.com/` | known_good | 新闻聚合 + 公告转载 |
| `jrj` | 金融界 | `https://stock.jrj.com.cn/` | known_good | 题材联动 / 盘面复盘 |
| `investing` | Investing.com | `https://www.investing.com/` | known_good | 商品 / 外汇 / 海外指数 / 宏观日历 |
| `mx_api` | 东方财富妙想 Skills Hub | `https://mkapi2.dfcfs.com/finskillshub/` | known_good | v2.3 新增 · 需 MX_APIKEY；官方 NLP API，自动纠错中文名 |
| `akshare_lhb` | akshare 龙虎榜 | `https://akshare.akfamily.xyz/` | known_good | ak.stock_lhb_detail_em 等；主 LHB 数据源 |
| `baostock` | BaoStock | `http://baostock.com/` | known_good | K 线 fallback，官方接口无 key |
| `yfinance` | Yahoo Finance | `https://finance.yahoo.com/` | known_good | 美股主源、港股兜底 |
| `ddgs` | DuckDuckGo 搜索 | `https://duckduckgo.com/` | flaky | 中文搜索质量不稳定；agent 建议二次过滤 garbage patterns |
| `yahoo_chart_v8` | Yahoo Finance Chart v8 (HTTP) | `https://query1.finance.yahoo.com/v8/finance/chart/` | known_good | 美股/港股 K 线直接 HTTP · 格式 ?symbol=AAPL&interval=1d&range=1mo · v7/quote 已被 Yahoo 关闭需 401 · v8 仍公开 |
| `yahoo_equity_screener` | Yahoo Finance 全球股票筛选 | `https://query2.finance.yahoo.com/v1/finance/screener` | known_good | 按 Yahoo 细分行业发现全球候选；结果仍需发行人去重、币种和数据完整度校验 |
| `yahoo_fundamentals_timeseries` | Yahoo Finance 全球年度财务时序 | `https://query2.finance.yahoo.com/ws/fundamentals-timeseries/v1/finance/timeseries/` | known_good | 全球同行年度营收/利润/权益/现金流；固定 host + symbol allowlist + 12s timeout |
| `yahoo_fx_chart` | Yahoo Finance 外汇 Chart | `https://query1.finance.yahoo.com/v8/finance/chart/JPYUSD=X` | known_good | 按年份计算汇率均值；原币报表值保留，换算值写入独立 *_base 字段 |
| `tencent_hk_quote` | 腾讯港股实时 qt.gtimg.cn | `http://qt.gtimg.cn/q=hk00700` | known_good | 港股实时行情 HK00700 类格式 · 腾讯自家接口无反爬 · 国内外都通 |
| `coingecko_simple_price` | CoinGecko Simple Price | `https://api.coingecko.com/api/v3/simple/price` | known_good | 加密货币实时价格 · 宏观风险偏好参考 · 参数 ?ids=bitcoin,ethereum&vs_currencies=usd |
| `coingecko_markets` | CoinGecko Markets | `https://api.coingecko.com/api/v3/coins/markets` | known_good | Top 100 加密货币行情 + 市值 · 可作宏观资金流参考 |
| `okx_spot_tickers` | OKX 现货 tickers (API v5) | `https://www.okx.com/api/v5/market/tickers?instType=SPOT` | known_good | OKX 国内访问不受限 · BTC/ETH/altcoin 全量现货快照 · 加密市场情绪代理 |
| `kucoin_stats` | KuCoin 24h 统计 | `https://api.kucoin.com/api/v1/market/stats` | known_good | 参数 ?symbol=BTC-USDT · 24h 涨跌 + 成交量 · 备用加密源 |
| `kraken_trades` | Kraken 公开成交 | `https://api.kraken.com/0/public/Trades` | known_good | 参数 ?pair=xbtusd · 近期成交流水 · 合规美金加密交易所 |
| `gemini_ticker` | Gemini 行情 | `https://api.gemini.com/v2/ticker/btcusd` | known_good | 合规美金加密交易所 · 美国用户主场 · 数据相对干净 |
| `coinlore_tickers` | CoinLore 全量币种 | `https://api.coinlore.net/api/tickers/` | known_good | 无分页限制 · 一次 36KB JSON · 适合加密市场全景快照 |
| `geckoterminal_networks` | GeckoTerminal DEX Networks | `https://api.geckoterminal.com/api/v2/networks` | known_good | DEX 数据 · Uniswap/PancakeSwap 等 · 链上资金流参考 |
| `jin10_flash` | 金十数据实时快讯 | `https://www.jin10.com/flash_newest.js` | known_good | 财联社替代品 · 实时快讯 JSON · 38KB · 含国内外宏观/政策/突发/行情 · akshare 也封装为 ak.js_news() |
| `em_kuaixun` | 东财快讯 (kuaixun) · 类财联社 | `https://newsapi.eastmoney.com/kuaixun/v1/getlist_102_ajaxResult_50_1_.html` | known_good | 东财快讯流 · 62KB · 含股票/宏观/政策/突发新闻 · 跟财联社风格相近 |
| `em_stock_ann` | 东财上市公司公告 | `https://np-anotice-stock.eastmoney.com/api/security/ann` | known_good | 公告 JSON 流 · 支持 page_size + ann_type 过滤 · 替代 cninfo 做高频轮询 |
| `qh99_inventory` | 99 期货网 · 库存/现货/基差 | `https://www.99qh.com/` | known_good | 中国最全期货库存/仓单/现货价/基差数据 · 需 HTML 解析 · 国内期货行业核心源 |
| `cfachina` | 中国期货业协会 | `http://www.cfachina.org/` | known_good | 期货业政策/法规/协会公告 · 权威官方 · 国内期货政策参考 |
| `ths_news_today` | 同花顺今日财经快讯 | `http://news.10jqka.com.cn/today_list/` | known_good | 同花顺实时快讯列表 · 68KB HTML 解析 · 财经/行情/行业快讯聚合 |

### Tier 2（11 条）

| id | 名称 | base_url | access | health | notes |
|---|---|---|---|---|---|
| `iwencai` | 问财（同花顺 NLP 筛选） | `https://www.iwencai.com/` | playwright | needs_browser | NLP 条件查询：'市值>100亿 行业=半导体'；需 cookie 流程 |
| `ths_f10` | 同花顺 F10 | `https://stockpage.10jqka.com.cn/` | playwright | needs_browser | 主营 / 股东 / 同行 / 概念板块映射，A 股信息最齐全 |
| `xueqiu_f10` | 雪球 F10 / 讨论 | `https://xueqiu.com/` | playwright | needs_browser | HTTP 直抓常返 403；用 Playwright 可稳定抓社区观点与公告 |
| `legulegu` | 乐咕乐股估值历史 | `https://legulegu.com/` | playwright | needs_browser | PE/PB 5Y 分位、行业估值；HTTP 直访返 403 |
| `stockstar` | 证券之星 | `https://stock.stockstar.com/` | playwright | needs_browser | 数据中心 + 研报评级；HTTP 直访返 567 |
| `futu` | 富途牛牛 | `https://www.futunn.com/` | playwright | needs_browser | 港美股页面 + 社区；HTTP 直访跳 403 |
| `yuncaijing` | 云财经龙虎榜 | `https://www.yuncaijing.com/` | playwright | flaky | 游资席位 / 题材热度 / 龙虎榜补源 |
| `guba_em_list` | 东财股吧 list 页（按股票代码） | `https://guba.eastmoney.com/list,{code}.html` | http | known_good | v2.7.3 新增 · list,{code}.html 200 OK 含真实帖子标题；600519/00700 验证可抓 |
| `jisilu` | 集思录 | `https://www.jisilu.cn/` | ddgs | flaky | v2.7.3 新增 · 社区观点 / 可转债/套利；站内搜索要会员，走 ddgs site:jisilu.cn |
| `fx678` | 汇通财经 | `https://www.fx678.com/` | ddgs | flaky | v2.7.3 新增 · 大宗商品 / 外汇 / 宏观快讯（列表路径要找，用 ddgs site: 查） |
| `cmc` | CompaniesMarketCap | `https://companiesmarketcap.com/` | http | known_good | v2.7.3 新增 · 英文站，港美股市值/估值 fallback；/tencent/marketcap/ 200 OK |

### Tier 3（21 条）

| id | 名称 | base_url | access | health | notes |
|---|---|---|---|---|---|
| `sse` | 上海证券交易所 | `https://www.sse.com.cn/` | http | known_good | 上交所披露 + 上证 e 互动 |
| `szse` | 深圳证券交易所 | `https://www.szse.cn/` | http | known_good | 深交所披露 + 互动易 |
| `csrc` | 中国证监会 | `http://www.csrc.gov.cn/` | http | known_good | 监管政策原文 |
| `gov_cn` | 国务院政策 | `https://www.gov.cn/zhengce/` | http | known_good | 顶层政策文件 |
| `miit` | 工信部 | `https://www.miit.gov.cn/` | http | known_good | 制造业行业政策 |
| `ndrc` | 发改委 | `https://www.ndrc.gov.cn/` | http | known_good | 发改委政策解读 |
| `samr` | 市场监管总局 | `https://www.samr.gov.cn/` | http | known_good | 反垄断 / 市场监管 |
| `shfe` | 上海期货交易所 | `https://www.shfe.com.cn/` | http | known_good | 黑色 / 有色 / 贵金属 / 原油期货日报 |
| `dce` | 大连商品交易所 | `https://www.dce.com.cn/` | http | known_good | 农产品 / 化工期货 |
| `czce` | 郑州商品交易所 | `https://www.czce.com.cn/` | http | known_good | 农产品 / 能源期货 |
| `100ppi` | 生意社现货 | `https://www.100ppi.com/` | http | known_good | 现货价格数据库 |
| `cnstock` | 中国证券网 | `https://www.cnstock.com/` | ddgs | known_good | v2.7.3 新增 · 上证 e 互动 / 新股报告 / 公司公告交叉验证。ddgs site:cnstock.com 验证返真实新闻标题 |
| `cs_cn` | 中证网 | `https://www.cs.com.cn/` | ddgs | known_good | v2.7.3 新增 · 中证报权威；ddgs site:cs.com.cn 返公司/政策真实新闻 |
| `stcn` | 证券时报 | `https://www.stcn.com/` | ddgs | known_good | v2.7.3 新增 · 证券时报网；ddgs site:stcn.com 返真实文章（如：腾讯控股回购） |
| `nbd` | 每日经济新闻 | `https://www.nbd.com.cn/` | ddgs | known_good | v2.7.3 新增 · 每经网产业/公司新闻；ddgs site:nbd.com.cn 返真实新闻 |
| `pbc` | 中国人民银行 | `http://www.pbc.gov.cn/` | ddgs | known_good | v2.7.3 新增 · 央行利率 / 货币政策原文；ddgs site:pbc.gov.cn |
| `safe` | 国家外汇管理局 | `https://www.safe.gov.cn/` | ddgs | known_good | v2.7.3 新增 · 外汇 / 跨境资金政策 |
| `stats_gov` | 国家统计局 | `http://www.stats.gov.cn/` | ddgs | known_good | v2.7.3 新增 · GDP / PMI / CPI / 工业增加值原始数据；ddgs site:stats.gov.cn |
| `chinamoney` | 中国货币网 | `https://www.chinamoney.com.cn/` | ddgs | known_good | v2.7.3 新增 · 银行间市场 / Shibor / CFETS |
| `chinabond` | 中国债券信息网 | `https://yield.chinabond.com.cn/` | http | known_good | v2.7.3 新增 · 国债收益率曲线（WACC 无风险利率锚）；首页 yield.chinabond.com.cn/ 200 OK |
| `ine` | 上海国际能源交易中心 | `https://www.ine.cn/` | http | known_good | v2.7.3 新增 · 原油/燃油/天然橡胶期货日报 |

注册表提供查询函数（`registry.rs`）：`by_id` / `by_dim` / `by_market` / `by_tier`，
以及按 tier 与 health 排序的 `http_sources_for` / `playwright_sources_for` / `official_sources_for`。
`assert_registry_sane()` 校验所有 id 唯一。

---

## Health Check（诊断）

查看当前所有 provider 的可用性：

```bash
cargo run -p uzi-data --example fetch_one -- --providers
```

输出示例（2026-09 实机）：

```
────────────────────────────────────────────────────────────
  Provider 健康度 (v2.10.6)
────────────────────────────────────────────────────────────

  name         avail    key req  markets     status
  ------------ -------- -------- ----------  ------------------------------
  akshare      ✓        no       A,H,U       ok
  baostock     ✗        no       A           unavailable
  direct_http  ✓        no       A,H,U       ok
  efinance     ✗        no       A,H,U       unavailable
  tushare      ✗        yes      A           unavailable
```

查 A 股每个维度的优先级链：

```bash
cargo run -p uzi-data --example fetch_one -- --providers chain A
```

输出示例（2026-09 实机，仅 akshare 可用时）：

```
────────────────────────────────────────────────────────────
  Provider 优先级链 · market=A
────────────────────────────────────────────────────────────
  kline          akshare
  financials     akshare
  basic          akshare
  lhb            akshare
```

只查单维度（`chain <market> <dim...>`），并可在前面加 `UZI_PROVIDERS_<DIM>` 观察覆盖效果：

```bash
UZI_PROVIDERS_KLINE=direct_http,akshare \
  cargo run -p uzi-data --example fetch_one -- --providers chain A kline
```

`--providers` 之后的参数原样转发给 `uzi_data::providers::cli::main`（`crates/uzi-data/src/providers/cli.rs`）。

---

## 环境变量

| 变量 | 作用 |
|---|---|
| `UZI_PROVIDERS_<DIM>` | 覆盖单个维度的 provider 优先级链（逗号分隔），如 `UZI_PROVIDERS_KLINE=direct_http` |
| `MX_APIKEY` | 启用 registry 的 `mx_api`（东方财富妙想 Skills Hub，官方 NLP API）；未设置时相关调用返回 `{"error": "MX_APIKEY not set"}` |
| `TUSHARE_TOKEN` | `tushare` provider 的 token；Rust 侧没有 Python transport，该 provider 仍恒为不可用 |
| `UZI_HTTP_TIMEOUT` | HTTP 超时（秒） |

---

## 建议配置

**默认（零配置）**：`akshare` + `direct_http` + `ddgs`，无需任何安装。A 股/港股/美股主要行情由
`akshare`（东财 HTTP）与 `direct_http`（腾讯/新浪）承担。

**启用 MX 妙想 API**（registry tier-1 的官方 NLP 源，自动纠错中文名）：

```bash
export MX_APIKEY=<你的 key>
```

**启用 Tushare**：本项目 provider 恒为 `unavailable`（无法在 Rust 中提供 Python transport），
链会自动跳过；`TUSHARE_TOKEN` 只影响 `token_present()`。上游 Python 版才是其可用形态。

**扩展一个新的 provider**：在 `crates/uzi-data/src/providers/` 加一个模块（实现
`NAME` / `REQUIRES_KEY` / `MARKETS` / `is_available()` 与对应 fetch 方法），并在
`providers/mod.rs` 的 `registry()` 中登记；纯 URL 目录项加进 `registry.rs` 的 `SOURCES`。

**付费源**（Wind / 万得 · Choice · iFinD · Bloomberg · Refinitiv）不默认接入，用户有授权时可自行接入。
