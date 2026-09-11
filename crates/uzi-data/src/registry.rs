//! Port of `lib/data_source_registry.py` — catalog of every data source
//! UZI-Skill knows about: URL, markets/dims covered, access method, health.
//!
//! Pure config, no I/O. `SOURCES` is `_TIER1 + _TIER2 + _TIER2_EXTRA_V273 + _TIER3`
//! in upstream declaration order (73 entries; 41 tier-1).

/// One catalogued data source.
pub struct DataSource {
    pub id: &'static str,
    pub name_cn: &'static str,
    pub base_url: &'static str,
    pub markets: &'static [&'static str],
    pub dims: &'static [&'static str],
    pub tier: u8,
    pub access: &'static str,
    pub health: &'static str,
    pub notes: &'static str,
}

/// `GLOBAL_MARKETS`.
pub const GLOBAL_MARKETS: &[&str] = &[
    "A", "H", "U", "JP", "KR", "TW", "SG", "IN", "CA", "AU", "GB", "DE", "FR", "NL", "CH",
    "ES", "IT", "SE", "NO", "DK", "FI", "BE", "PT", "BR", "MX", "TH", "ID", "MY", "NZ", "ZA",
    "IL", "G",
];

/// `SOURCES`.
pub static SOURCES: &[DataSource] = &[
    DataSource {
        id: "em_push2",
        name_cn: "东方财富 push2",
        base_url: "https://push2.eastmoney.com/api/qt/stock/get",
        markets: &["A", "H", "U"],
        dims: &["0_basic", "2_kline", "10_valuation", "12_capital_flow"],
        tier: 1,
        access: "http",
        health: "blocked_often",
        notes: "2026 常被反爬拦截（大陆 / 境外均可能 Empty reply）；建议走 MX API 或 XueQiu akshare 代抓",
    },
    DataSource {
        id: "em_quote",
        name_cn: "东方财富 quote 页",
        base_url: "https://quote.eastmoney.com/",
        markets: &["A", "H", "U"],
        dims: &["0_basic", "2_kline"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "push2 挂掉时 quote 子域通常仍可用（2026 验证：200 OK）",
    },
    DataSource {
        id: "em_data",
        name_cn: "东方财富 data 子域",
        base_url: "https://data.eastmoney.com/",
        markets: &["A"],
        dims: &["4_peers", "7_industry", "11_governance", "12_capital_flow", "16_lhb", "15_events", "6_research"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "龙虎榜 / 北向 / 融资融券 / 股东户数 / 研报 / 行业板块成分（akshare board_industry_cons_em 走这里）",
    },
    DataSource {
        id: "xq_api",
        name_cn: "雪球 akshare backend",
        base_url: "https://stock.xueqiu.com/",
        markets: &["A", "H"],
        dims: &["0_basic", "1_financials", "2_kline", "15_events", "17_sentiment"],
        tier: 1,
        access: "akshare",
        health: "known_good",
        notes: "akshare.stock_individual_basic_info_xq / stock_individual_spot_xq",
    },
    DataSource {
        id: "tencent_qt",
        name_cn: "腾讯行情 qt",
        base_url: "https://qt.gtimg.cn/",
        markets: &["A", "H", "U"],
        dims: &["0_basic", "2_kline"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "realtime quote 兜底源；格式 ~-delimited 字符串",
    },
    DataSource {
        id: "sina_quote",
        name_cn: "新浪财经行情",
        base_url: "https://finance.sina.com.cn/",
        markets: &["A", "H", "U"],
        dims: &["0_basic", "2_kline", "15_events"],
        tier: 1,
        access: "http",
        health: "flaky",
        notes: "hq.sinajs.cn 老接口 2026 返 403；主页 HTML 解析仍可用",
    },
    DataSource {
        id: "cninfo",
        name_cn: "巨潮资讯",
        base_url: "http://www.cninfo.com.cn/",
        markets: &["A"],
        dims: &["15_events", "7_industry", "1_financials"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "A 股公告原文的法定披露源；akshare.stock_industry_pe_ratio 也走这里",
    },
    DataSource {
        id: "hkexnews",
        name_cn: "HKEXNews 港交所披露易",
        base_url: "https://www1.hkexnews.hk/",
        markets: &["H"],
        dims: &["15_events", "11_governance"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "港股公告的法定披露源",
    },
    DataSource {
        id: "aastocks",
        name_cn: "AASTOCKS 港股",
        base_url: "https://www.aastocks.com/",
        markets: &["H"],
        dims: &["0_basic", "4_peers", "12_capital_flow", "15_events"],
        tier: 1,
        access: "http",
        health: "flaky",
        notes: "港股 PE/PB/industry/南北向核心数据源；HTML regex 抓取 + Playwright 兜底",
    },
    DataSource {
        id: "cls",
        name_cn: "财联社 7x24 电报",
        base_url: "https://www.cls.cn/",
        markets: &["A", "H", "U"],
        dims: &["15_events", "3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "事件驱动首选；催化剂与突发新闻密度最高",
    },
    DataSource {
        id: "yicai",
        name_cn: "第一财经",
        base_url: "https://www.yicai.com/",
        markets: &["A", "H"],
        dims: &["15_events", "3_macro", "7_industry"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "行业与公司新闻、宏观产业专题；适合 agent 抽取定性评语",
    },
    DataSource {
        id: "wallstreetcn",
        name_cn: "华尔街见闻",
        base_url: "https://wallstreetcn.com/",
        markets: &["A", "H", "U"],
        dims: &["3_macro", "17_sentiment"],
        tier: 1,
        access: "http",
        health: "flaky",
        notes: "快讯 + 海外联动；/live 端点 2026 返 404，走主页抓最新",
    },
    DataSource {
        id: "cfi",
        name_cn: "中财网",
        base_url: "https://quote.cfi.cn/",
        markets: &["A"],
        dims: &["0_basic", "1_financials", "15_events"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "个股资料 / 公告 / 研报 HTML 兜底",
    },
    DataSource {
        id: "hexun",
        name_cn: "和讯网",
        base_url: "https://stock.hexun.com/",
        markets: &["A"],
        dims: &["6_research", "15_events"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "研报转载 + 行业点评兜底",
    },
    DataSource {
        id: "163money",
        name_cn: "网易财经",
        base_url: "https://money.163.com/",
        markets: &["A", "U"],
        dims: &["0_basic", "15_events"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "新闻聚合 + 公告转载",
    },
    DataSource {
        id: "jrj",
        name_cn: "金融界",
        base_url: "https://stock.jrj.com.cn/",
        markets: &["A"],
        dims: &["15_events", "7_industry"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "题材联动 / 盘面复盘",
    },
    DataSource {
        id: "investing",
        name_cn: "Investing.com",
        base_url: "https://www.investing.com/",
        markets: &["U"],
        dims: &["3_macro", "9_futures"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "商品 / 外汇 / 海外指数 / 宏观日历",
    },
    DataSource {
        id: "mx_api",
        name_cn: "东方财富妙想 Skills Hub",
        base_url: "https://mkapi2.dfcfs.com/finskillshub/",
        markets: &["A", "H", "U"],
        dims: &["0_basic", "1_financials", "15_events"],
        tier: 1,
        access: "mx_api",
        health: "known_good",
        notes: "v2.3 新增 · 需 MX_APIKEY；官方 NLP API，自动纠错中文名",
    },
    DataSource {
        id: "akshare_lhb",
        name_cn: "akshare 龙虎榜",
        base_url: "https://akshare.akfamily.xyz/",
        markets: &["A"],
        dims: &["16_lhb"],
        tier: 1,
        access: "akshare",
        health: "known_good",
        notes: "ak.stock_lhb_detail_em 等；主 LHB 数据源",
    },
    DataSource {
        id: "baostock",
        name_cn: "BaoStock",
        base_url: "http://baostock.com/",
        markets: &["A"],
        dims: &["2_kline"],
        tier: 1,
        access: "akshare",
        health: "known_good",
        notes: "K 线 fallback，官方接口无 key",
    },
    DataSource {
        id: "yfinance",
        name_cn: "Yahoo Finance",
        base_url: "https://finance.yahoo.com/",
        markets: &["U", "H"],
        dims: &["0_basic", "1_financials", "2_kline"],
        tier: 1,
        access: "akshare",
        health: "known_good",
        notes: "美股主源、港股兜底",
    },
    DataSource {
        id: "ddgs",
        name_cn: "DuckDuckGo 搜索",
        base_url: "https://duckduckgo.com/",
        markets: &["A", "H", "U"],
        dims: &["3_macro", "13_policy", "14_moat", "15_events", "17_sentiment"],
        tier: 1,
        access: "ddgs",
        health: "flaky",
        notes: "中文搜索质量不稳定；agent 建议二次过滤 garbage patterns",
    },
    DataSource {
        id: "yahoo_chart_v8",
        name_cn: "Yahoo Finance Chart v8 (HTTP)",
        base_url: "https://query1.finance.yahoo.com/v8/finance/chart/",
        markets: &["U", "H"],
        dims: &["2_kline"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "美股/港股 K 线直接 HTTP · 格式 ?symbol=AAPL&interval=1d&range=1mo · v7/quote 已被 Yahoo 关闭需 401 · v8 仍公开",
    },
    DataSource {
        id: "yahoo_equity_screener",
        name_cn: "Yahoo Finance 全球股票筛选",
        base_url: "https://query2.finance.yahoo.com/v1/finance/screener",
        markets: &["A", "H", "U", "JP", "KR", "TW", "SG", "IN", "CA", "AU", "GB", "DE", "FR", "NL", "CH", "ES", "IT", "SE", "NO", "DK", "FI", "BE", "PT", "BR", "MX", "TH", "ID", "MY", "NZ", "ZA", "IL", "G"],
        dims: &["4_peers"],
        tier: 1,
        access: "yfinance",
        health: "known_good",
        notes: "按 Yahoo 细分行业发现全球候选；结果仍需发行人去重、币种和数据完整度校验",
    },
    DataSource {
        id: "yahoo_fundamentals_timeseries",
        name_cn: "Yahoo Finance 全球年度财务时序",
        base_url: "https://query2.finance.yahoo.com/ws/fundamentals-timeseries/v1/finance/timeseries/",
        markets: &["A", "H", "U", "JP", "KR", "TW", "SG", "IN", "CA", "AU", "GB", "DE", "FR", "NL", "CH", "ES", "IT", "SE", "NO", "DK", "FI", "BE", "PT", "BR", "MX", "TH", "ID", "MY", "NZ", "ZA", "IL", "G"],
        dims: &["1_financials", "4_peers", "10_valuation"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "全球同行年度营收/利润/权益/现金流；固定 host + symbol allowlist + 12s timeout",
    },
    DataSource {
        id: "yahoo_fx_chart",
        name_cn: "Yahoo Finance 外汇 Chart",
        base_url: "https://query1.finance.yahoo.com/v8/finance/chart/JPYUSD=X",
        markets: &["A", "H", "U", "JP", "KR", "TW", "SG", "IN", "CA", "AU", "GB", "DE", "FR", "NL", "CH", "ES", "IT", "SE", "NO", "DK", "FI", "BE", "PT", "BR", "MX", "TH", "ID", "MY", "NZ", "ZA", "IL", "G"],
        dims: &["4_peers"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "按年份计算汇率均值；原币报表值保留，换算值写入独立 *_base 字段",
    },
    DataSource {
        id: "tencent_hk_quote",
        name_cn: "腾讯港股实时 qt.gtimg.cn",
        base_url: "http://qt.gtimg.cn/q=hk00700",
        markets: &["H"],
        dims: &["0_basic"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "港股实时行情 HK00700 类格式 · 腾讯自家接口无反爬 · 国内外都通",
    },
    DataSource {
        id: "coingecko_simple_price",
        name_cn: "CoinGecko Simple Price",
        base_url: "https://api.coingecko.com/api/v3/simple/price",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "加密货币实时价格 · 宏观风险偏好参考 · 参数 ?ids=bitcoin,ethereum&vs_currencies=usd",
    },
    DataSource {
        id: "coingecko_markets",
        name_cn: "CoinGecko Markets",
        base_url: "https://api.coingecko.com/api/v3/coins/markets",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "Top 100 加密货币行情 + 市值 · 可作宏观资金流参考",
    },
    DataSource {
        id: "okx_spot_tickers",
        name_cn: "OKX 现货 tickers (API v5)",
        base_url: "https://www.okx.com/api/v5/market/tickers?instType=SPOT",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "OKX 国内访问不受限 · BTC/ETH/altcoin 全量现货快照 · 加密市场情绪代理",
    },
    DataSource {
        id: "kucoin_stats",
        name_cn: "KuCoin 24h 统计",
        base_url: "https://api.kucoin.com/api/v1/market/stats",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "参数 ?symbol=BTC-USDT · 24h 涨跌 + 成交量 · 备用加密源",
    },
    DataSource {
        id: "kraken_trades",
        name_cn: "Kraken 公开成交",
        base_url: "https://api.kraken.com/0/public/Trades",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "参数 ?pair=xbtusd · 近期成交流水 · 合规美金加密交易所",
    },
    DataSource {
        id: "gemini_ticker",
        name_cn: "Gemini 行情",
        base_url: "https://api.gemini.com/v2/ticker/btcusd",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "合规美金加密交易所 · 美国用户主场 · 数据相对干净",
    },
    DataSource {
        id: "coinlore_tickers",
        name_cn: "CoinLore 全量币种",
        base_url: "https://api.coinlore.net/api/tickers/",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "无分页限制 · 一次 36KB JSON · 适合加密市场全景快照",
    },
    DataSource {
        id: "geckoterminal_networks",
        name_cn: "GeckoTerminal DEX Networks",
        base_url: "https://api.geckoterminal.com/api/v2/networks",
        markets: &["U"],
        dims: &["3_macro"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "DEX 数据 · Uniswap/PancakeSwap 等 · 链上资金流参考",
    },
    DataSource {
        id: "jin10_flash",
        name_cn: "金十数据实时快讯",
        base_url: "https://www.jin10.com/flash_newest.js",
        markets: &["A", "H", "U"],
        dims: &["3_macro", "13_policy", "15_events", "17_sentiment"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "财联社替代品 · 实时快讯 JSON · 38KB · 含国内外宏观/政策/突发/行情 · akshare 也封装为 ak.js_news()",
    },
    DataSource {
        id: "em_kuaixun",
        name_cn: "东财快讯 (kuaixun) · 类财联社",
        base_url: "https://newsapi.eastmoney.com/kuaixun/v1/getlist_102_ajaxResult_50_1_.html",
        markets: &["A", "H", "U"],
        dims: &["15_events", "17_sentiment"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "东财快讯流 · 62KB · 含股票/宏观/政策/突发新闻 · 跟财联社风格相近",
    },
    DataSource {
        id: "em_stock_ann",
        name_cn: "东财上市公司公告",
        base_url: "https://np-anotice-stock.eastmoney.com/api/security/ann",
        markets: &["A"],
        dims: &["15_events"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "公告 JSON 流 · 支持 page_size + ann_type 过滤 · 替代 cninfo 做高频轮询",
    },
    DataSource {
        id: "qh99_inventory",
        name_cn: "99 期货网 · 库存/现货/基差",
        base_url: "https://www.99qh.com/",
        markets: &["A"],
        dims: &["8_materials", "9_futures"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "中国最全期货库存/仓单/现货价/基差数据 · 需 HTML 解析 · 国内期货行业核心源",
    },
    DataSource {
        id: "cfachina",
        name_cn: "中国期货业协会",
        base_url: "http://www.cfachina.org/",
        markets: &["A"],
        dims: &["9_futures", "13_policy"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "期货业政策/法规/协会公告 · 权威官方 · 国内期货政策参考",
    },
    DataSource {
        id: "ths_news_today",
        name_cn: "同花顺今日财经快讯",
        base_url: "http://news.10jqka.com.cn/today_list/",
        markets: &["A", "H"],
        dims: &["15_events", "17_sentiment"],
        tier: 1,
        access: "http",
        health: "known_good",
        notes: "同花顺实时快讯列表 · 68KB HTML 解析 · 财经/行情/行业快讯聚合",
    },
    DataSource {
        id: "iwencai",
        name_cn: "问财（同花顺 NLP 筛选）",
        base_url: "https://www.iwencai.com/",
        markets: &["A"],
        dims: &["4_peers", "5_chain", "7_industry"],
        tier: 2,
        access: "playwright",
        health: "needs_browser",
        notes: "NLP 条件查询：'市值>100亿 行业=半导体'；需 cookie 流程",
    },
    DataSource {
        id: "ths_f10",
        name_cn: "同花顺 F10",
        base_url: "https://stockpage.10jqka.com.cn/",
        markets: &["A", "H"],
        dims: &["0_basic", "4_peers", "5_chain", "11_governance", "14_moat"],
        tier: 2,
        access: "playwright",
        health: "needs_browser",
        notes: "主营 / 股东 / 同行 / 概念板块映射，A 股信息最齐全",
    },
    DataSource {
        id: "xueqiu_f10",
        name_cn: "雪球 F10 / 讨论",
        base_url: "https://xueqiu.com/",
        markets: &["A", "H", "U"],
        dims: &["11_governance", "15_events", "17_sentiment"],
        tier: 2,
        access: "playwright",
        health: "needs_browser",
        notes: "HTTP 直抓常返 403；用 Playwright 可稳定抓社区观点与公告",
    },
    DataSource {
        id: "legulegu",
        name_cn: "乐咕乐股估值历史",
        base_url: "https://legulegu.com/",
        markets: &["A"],
        dims: &["10_valuation", "7_industry"],
        tier: 2,
        access: "playwright",
        health: "needs_browser",
        notes: "PE/PB 5Y 分位、行业估值；HTTP 直访返 403",
    },
    DataSource {
        id: "stockstar",
        name_cn: "证券之星",
        base_url: "https://stock.stockstar.com/",
        markets: &["A"],
        dims: &["6_research", "15_events"],
        tier: 2,
        access: "playwright",
        health: "needs_browser",
        notes: "数据中心 + 研报评级；HTTP 直访返 567",
    },
    DataSource {
        id: "futu",
        name_cn: "富途牛牛",
        base_url: "https://www.futunn.com/",
        markets: &["H", "U"],
        dims: &["0_basic", "1_financials", "17_sentiment"],
        tier: 2,
        access: "playwright",
        health: "needs_browser",
        notes: "港美股页面 + 社区；HTTP 直访跳 403",
    },
    DataSource {
        id: "yuncaijing",
        name_cn: "云财经龙虎榜",
        base_url: "https://www.yuncaijing.com/",
        markets: &["A"],
        dims: &["16_lhb"],
        tier: 2,
        access: "playwright",
        health: "flaky",
        notes: "游资席位 / 题材热度 / 龙虎榜补源",
    },
    DataSource {
        id: "sse",
        name_cn: "上海证券交易所",
        base_url: "https://www.sse.com.cn/",
        markets: &["A"],
        dims: &["15_events", "11_governance"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "上交所披露 + 上证 e 互动",
    },
    DataSource {
        id: "szse",
        name_cn: "深圳证券交易所",
        base_url: "https://www.szse.cn/",
        markets: &["A"],
        dims: &["15_events", "11_governance"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "深交所披露 + 互动易",
    },
    DataSource {
        id: "csrc",
        name_cn: "中国证监会",
        base_url: "http://www.csrc.gov.cn/",
        markets: &["A"],
        dims: &["13_policy"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "监管政策原文",
    },
    DataSource {
        id: "gov_cn",
        name_cn: "国务院政策",
        base_url: "https://www.gov.cn/zhengce/",
        markets: &["A", "H"],
        dims: &["13_policy", "3_macro"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "顶层政策文件",
    },
    DataSource {
        id: "miit",
        name_cn: "工信部",
        base_url: "https://www.miit.gov.cn/",
        markets: &["A"],
        dims: &["13_policy", "7_industry"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "制造业行业政策",
    },
    DataSource {
        id: "ndrc",
        name_cn: "发改委",
        base_url: "https://www.ndrc.gov.cn/",
        markets: &["A"],
        dims: &["13_policy", "3_macro"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "发改委政策解读",
    },
    DataSource {
        id: "samr",
        name_cn: "市场监管总局",
        base_url: "https://www.samr.gov.cn/",
        markets: &["A"],
        dims: &["13_policy"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "反垄断 / 市场监管",
    },
    DataSource {
        id: "shfe",
        name_cn: "上海期货交易所",
        base_url: "https://www.shfe.com.cn/",
        markets: &["A"],
        dims: &["8_materials", "9_futures"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "黑色 / 有色 / 贵金属 / 原油期货日报",
    },
    DataSource {
        id: "dce",
        name_cn: "大连商品交易所",
        base_url: "https://www.dce.com.cn/",
        markets: &["A"],
        dims: &["8_materials", "9_futures"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "农产品 / 化工期货",
    },
    DataSource {
        id: "czce",
        name_cn: "郑州商品交易所",
        base_url: "https://www.czce.com.cn/",
        markets: &["A"],
        dims: &["8_materials", "9_futures"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "农产品 / 能源期货",
    },
    DataSource {
        id: "100ppi",
        name_cn: "生意社现货",
        base_url: "https://www.100ppi.com/",
        markets: &["A"],
        dims: &["8_materials"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "现货价格数据库",
    },
    DataSource {
        id: "cnstock",
        name_cn: "中国证券网",
        base_url: "https://www.cnstock.com/",
        markets: &["A", "H"],
        dims: &["15_events", "6_research", "17_sentiment", "13_policy"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 上证 e 互动 / 新股报告 / 公司公告交叉验证。ddgs site:cnstock.com 验证返真实新闻标题",
    },
    DataSource {
        id: "cs_cn",
        name_cn: "中证网",
        base_url: "https://www.cs.com.cn/",
        markets: &["A", "H"],
        dims: &["15_events", "13_policy", "17_sentiment"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 中证报权威；ddgs site:cs.com.cn 返公司/政策真实新闻",
    },
    DataSource {
        id: "stcn",
        name_cn: "证券时报",
        base_url: "https://www.stcn.com/",
        markets: &["A", "H"],
        dims: &["15_events", "17_sentiment", "13_policy"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 证券时报网；ddgs site:stcn.com 返真实文章（如：腾讯控股回购）",
    },
    DataSource {
        id: "nbd",
        name_cn: "每日经济新闻",
        base_url: "https://www.nbd.com.cn/",
        markets: &["A", "H", "U"],
        dims: &["15_events", "17_sentiment", "18_trap", "14_moat"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 每经网产业/公司新闻；ddgs site:nbd.com.cn 返真实新闻",
    },
    DataSource {
        id: "pbc",
        name_cn: "中国人民银行",
        base_url: "http://www.pbc.gov.cn/",
        markets: &["A", "H"],
        dims: &["3_macro", "13_policy"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 央行利率 / 货币政策原文；ddgs site:pbc.gov.cn",
    },
    DataSource {
        id: "safe",
        name_cn: "国家外汇管理局",
        base_url: "https://www.safe.gov.cn/",
        markets: &["A", "H", "U"],
        dims: &["3_macro", "13_policy"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 外汇 / 跨境资金政策",
    },
    DataSource {
        id: "stats_gov",
        name_cn: "国家统计局",
        base_url: "http://www.stats.gov.cn/",
        markets: &["A", "H"],
        dims: &["3_macro", "7_industry"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · GDP / PMI / CPI / 工业增加值原始数据；ddgs site:stats.gov.cn",
    },
    DataSource {
        id: "chinamoney",
        name_cn: "中国货币网",
        base_url: "https://www.chinamoney.com.cn/",
        markets: &["A"],
        dims: &["3_macro", "12_capital_flow"],
        tier: 3,
        access: "ddgs",
        health: "known_good",
        notes: "v2.7.3 新增 · 银行间市场 / Shibor / CFETS",
    },
    DataSource {
        id: "chinabond",
        name_cn: "中国债券信息网",
        base_url: "https://yield.chinabond.com.cn/",
        markets: &["A"],
        dims: &["3_macro", "10_valuation"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "v2.7.3 新增 · 国债收益率曲线（WACC 无风险利率锚）；首页 yield.chinabond.com.cn/ 200 OK",
    },
    DataSource {
        id: "ine",
        name_cn: "上海国际能源交易中心",
        base_url: "https://www.ine.cn/",
        markets: &["A"],
        dims: &["8_materials", "9_futures"],
        tier: 3,
        access: "http",
        health: "known_good",
        notes: "v2.7.3 新增 · 原油/燃油/天然橡胶期货日报",
    },
    DataSource {
        id: "guba_em_list",
        name_cn: "东财股吧 list 页（按股票代码）",
        base_url: "https://guba.eastmoney.com/list,{code}.html",
        markets: &["A", "H"],
        dims: &["17_sentiment", "18_trap", "19_contests"],
        tier: 2,
        access: "http",
        health: "known_good",
        notes: "v2.7.3 新增 · list,{code}.html 200 OK 含真实帖子标题；600519/00700 验证可抓",
    },
    DataSource {
        id: "jisilu",
        name_cn: "集思录",
        base_url: "https://www.jisilu.cn/",
        markets: &["A"],
        dims: &["17_sentiment", "19_contests"],
        tier: 2,
        access: "ddgs",
        health: "flaky",
        notes: "v2.7.3 新增 · 社区观点 / 可转债/套利；站内搜索要会员，走 ddgs site:jisilu.cn",
    },
    DataSource {
        id: "fx678",
        name_cn: "汇通财经",
        base_url: "https://www.fx678.com/",
        markets: &["A", "U"],
        dims: &["3_macro", "8_materials", "9_futures"],
        tier: 2,
        access: "ddgs",
        health: "flaky",
        notes: "v2.7.3 新增 · 大宗商品 / 外汇 / 宏观快讯（列表路径要找，用 ddgs site: 查）",
    },
    DataSource {
        id: "cmc",
        name_cn: "CompaniesMarketCap",
        base_url: "https://companiesmarketcap.com/",
        markets: &["H", "U"],
        dims: &["0_basic", "10_valuation"],
        tier: 2,
        access: "http",
        health: "known_good",
        notes: "v2.7.3 新增 · 英文站，港美股市值/估值 fallback；/tencent/marketcap/ 200 OK",
    },];

/// `by_id(source_id)`.
pub fn by_id(source_id: &str) -> Option<&'static DataSource> {
    SOURCES.iter().find(|s| s.id == source_id)
}

/// `by_dim(dim_key)`.
pub fn by_dim(dim_key: &str) -> Vec<&'static DataSource> {
    SOURCES.iter().filter(|s| s.dims.contains(&dim_key)).collect()
}

/// `by_market(market)`.
pub fn by_market(market: &str) -> Vec<&'static DataSource> {
    SOURCES.iter().filter(|s| s.markets.contains(&market)).collect()
}

/// `by_tier(tier)`.
pub fn by_tier(tier: u8) -> Vec<&'static DataSource> {
    SOURCES.iter().filter(|s| s.tier == tier).collect()
}

/// `http_sources_for(dim_key, market)` — tier-1, health-ordered
/// (`known_good` → `flaky` → `blocked_often` → `needs_browser`).
pub fn http_sources_for(dim_key: &str, market: &str) -> Vec<&'static DataSource> {
    let rank = |h: &str| match h {
        "known_good" => 0,
        "flaky" => 1,
        "blocked_often" => 2,
        "needs_browser" => 3,
        _ => 99,
    };
    let mut hits: Vec<&'static DataSource> = SOURCES
        .iter()
        .filter(|s| s.tier == 1 && s.markets.contains(&market) && s.dims.contains(&dim_key))
        .collect();
    hits.sort_by_key(|s| rank(s.health));
    hits
}

/// `playwright_sources_for(dim_key, market)` — tier-2.
pub fn playwright_sources_for(dim_key: &str, market: &str) -> Vec<&'static DataSource> {
    SOURCES
        .iter()
        .filter(|s| s.tier == 2 && s.markets.contains(&market) && s.dims.contains(&dim_key))
        .collect()
}

/// `official_sources_for(dim_key)` — tier-3.
pub fn official_sources_for(dim_key: &str) -> Vec<&'static DataSource> {
    SOURCES
        .iter()
        .filter(|s| s.tier == 3 && s.dims.contains(&dim_key))
        .collect()
}

/// `assert_registry_sane()` — every id is unique.
pub fn assert_registry_sane() -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::new();
    for s in SOURCES {
        if seen.contains(&s.id) {
            return Err(format!("Duplicate source IDs: {}", s.id));
        }
        seen.push(s.id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_no_duplicate_ids() {
        assert_registry_sane().unwrap();
        assert_eq!(SOURCES.len(), 73);
    }

    #[test]
    fn lookups_match_upstream_examples() {
        assert!(by_id("em_push2").is_some());
        assert!(by_id("no_such_source").is_none());
        assert!(!by_dim("4_peers").is_empty());
        assert!(by_market("H").iter().all(|s| s.markets.contains(&"H")));
        assert_eq!(by_tier(1).len(), 41);
    }

    #[test]
    fn http_sources_are_health_ordered() {
        let hits = http_sources_for("0_basic", "A");
        assert!(!hits.is_empty());
        let rank = |h: &str| match h {
            "known_good" => 0,
            "flaky" => 1,
            "blocked_often" => 2,
            _ => 3,
        };
        let ranks: Vec<u8> = hits.iter().map(|s| rank(s.health)).collect();
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]));
        assert!(hits.iter().all(|s| s.tier == 1));
    }

    #[test]
    fn tier3_official_sources_are_market_agnostic() {
        assert!(official_sources_for("15_events").iter().all(|s| s.tier == 3));
    }
}
