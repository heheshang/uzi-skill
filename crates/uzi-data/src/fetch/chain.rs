//! Port of `fetch_chain.py`.
//!
//! Upstream's two sources are AkShare wrappers over documented EastMoney /
//! 同花顺 endpoints: `stock_zygc_em` → `emweb.securities.eastmoney.com/PC_HSF10/
//! BusinessAnalysis/PageAjax`, and `stock_zyjs_ths` → `basic.10jqka.com.cn/new/
//! {code}/operate.html`. Both are fetched directly here; the 上下游 inference is
//! the same keyword heuristic as upstream.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use uzi_core::ticker::parse_ticker;

const THS_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/109.0.0.0 Safari/537.36";

fn first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `_float(v)` — `float(str(v).replace("%","").replace(",",""))`, else 0.0.
fn float_or(v: &Value) -> f64 {
    let s = match v {
        Value::Null => return 0.0,
        Value::Number(n) => return n.as_f64().unwrap_or(0.0),
        Value::Bool(_) => return 0.0,
        Value::String(s) => s.clone(),
        _ => return 0.0,
    };
    let cleaned: String = s.chars().filter(|c| *c != '%' && *c != ',').collect();
    if cleaned.is_empty() || matches!(cleaned.as_str(), "nan" | "-") {
        return 0.0;
    }
    cleaned.parse::<f64>().unwrap_or(0.0)
}

fn num_or_null(v: Option<&Value>) -> Value {
    match v {
        None => Value::Null,
        Some(Value::Null) => Value::Null,
        Some(Value::Number(n)) => Value::Number(n.clone()),
        Some(Value::String(s)) => s
            .parse::<f64>()
            .ok()
            .and_then(|f| serde_json::Number::from_f64(f))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(_) => Value::Null,
    }
}

/// `ak.stock_zygc_em(symbol)` → renamed records, or `Err` on the upstream
/// exception path.
fn fetch_zygc(sym: &str) -> Result<Vec<Value>, String> {
    let url = "https://emweb.securities.eastmoney.com/PC_HSF10/BusinessAnalysis/PageAjax";
    let v = crate::http::get_json_q(url, &[("code", sym)], &[], 15)?;
    let rows = v
        .get("zygcfx")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let out = rows
        .iter()
        .map(|r| {
            let report_date = r
                .get("REPORT_DATE")
                .and_then(|v| v.as_str())
                .map(|s| first_n(s, 10))
                .unwrap_or_default();
            let category = match r.get("MAINOP_TYPE").and_then(|v| v.as_str()) {
                Some("1") => json!("按行业分类"),
                Some("2") => json!("按产品分类"),
                Some("3") => json!("按地区分类"),
                _ => Value::Null,
            };
            json!({
                "股票代码": r.get("SECURITY_CODE").cloned().unwrap_or(Value::Null),
                "报告日期": report_date,
                "分类类型": category,
                "主营构成": r.get("ITEM_NAME").cloned().unwrap_or(Value::Null),
                "主营收入": num_or_null(r.get("MAIN_BUSINESS_INCOME")),
                "收入比例": num_or_null(r.get("MBI_RATIO")),
                "主营成本": num_or_null(r.get("MAIN_BUSINESS_COST")),
                "成本比例": num_or_null(r.get("MBC_RATIO")),
                "主营利润": num_or_null(r.get("MAIN_BUSINESS_RPOFIT")),
                "利润比例": num_or_null(r.get("MBR_RATIO")),
                "毛利率": num_or_null(r.get("GROSS_RPOFIT_RATIO")),
            })
        })
        .collect();
    Ok(out)
}

/// Breakdown: latest report period, `主营构成` names against `收入比例`
/// (upstream reads the raw fraction column, not a percentage).
fn build_breakdown(records: &[Value]) -> Vec<Value> {
    let Some(latest_date) = records.first().and_then(|r| r.get("报告日期")) else {
        return Vec::new();
    };
    let df_latest: Vec<&Value> = records
        .iter()
        .filter(|r| r.get("报告日期") == Some(latest_date))
        .collect();

    let mut items: Vec<(String, f64)> = Vec::new();
    for row in df_latest {
        let name = row
            .get("主营构成")
            .map(uzi_core::py::py_str)
            .unwrap_or_default();
        if name.is_empty() || name == "nan" || name == "合计" || name == "总计" {
            continue;
        }
        let v = float_or(row.get("收入比例").unwrap_or(&Value::Null));
        if v > 0.0 {
            items.push((first_n(&name, 12), uzi_core::py::round(v, 1)));
        }
    }
    items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    items
        .into_iter()
        .take(6)
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect()
}

/// `ak.stock_zyjs_ths(symbol)` → the four main-intro fields, or `Err`.
fn fetch_zyjs(code: &str) -> Result<Map<String, Value>, String> {
    static UL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<ul class="main_intro_list">(.*?)</ul>"#).unwrap()
    });
    static LI: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)<li><span[^>]*>(.*?)</span><p>(.*?)</p></li>").unwrap()
    });

    let url = format!("https://basic.10jqka.com.cn/new/{code}/operate.html");
    let resp = crate::http::get(&url, &[("User-Agent", THS_UA)], 15)?;
    if !resp.is_ok() {
        return Err(format!("HTTP {}", resp.status));
    }
    let html = resp.gbk_text();
    let ul = UL
        .captures(&html)
        .ok_or_else(|| "main_intro_list missing".to_string())?;
    let inner = ul.get(1).map(|m| m.as_str()).unwrap_or("");

    let mut fields: Vec<(String, String)> = Vec::new();
    for cap in LI.captures_iter(inner) {
        let key_raw = &cap[1];
        let key = key_raw.split('：').next().unwrap_or("").trim().to_string();
        let value: String = cap[2]
            .chars()
            .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
            .collect();
        fields.push((key, value));
    }
    let get = |k: &str| -> String {
        fields
            .iter()
            .find(|(key, _)| key.as_str() == k)
            .map(|(_, v)| v.to_string())
            .unwrap_or_default()
    };
    let mut out = Map::new();
    out.insert("主营业务".into(), json!(get("主营业务")));
    out.insert("产品类型".into(), json!(get("产品类型")));
    out.insert("产品名称".into(), json!(get("产品名称")));
    out.insert("经营范围".into(), json!(first_n(&get("经营范围"), 200)));
    Ok(out)
}

/// `_UPSTREAM_HINTS` — first keyword hit in `f"{biz} {prod} {scope}"` wins.
const UPSTREAM_HINTS: &[(&str, &str)] = &[
    ("港口", "航运公司、进出口贸易商、物流企业"),
    ("航运", "造船厂、燃油供应商、港口服务"),
    ("建筑", "水泥/钢材/砂石供应商、劳务分包商"),
    ("房地产", "建材供应商、建筑承包商、设计院"),
    ("汽车", "零部件供应商、钢铁/铝材、电子元器件"),
    ("电池", "正极/负极/电解液/隔膜材料供应商"),
    ("半导体", "晶圆代工、光刻机、EDA 工具、材料"),
    ("医药", "原料药供应商、CRO/CDMO、包装材料"),
    ("白酒", "粮食采购、包装材料、物流"),
    ("光伏", "硅料/硅片/电池片供应商"),
    ("钢铁", "铁矿石/焦炭供应商"),
    ("煤炭", "采矿设备、运输物流"),
    ("银行", "央行/同业资金、存款客户"),
    ("保险", "再保险公司、精算/IT 服务商"),
    ("电力", "煤炭/天然气供应商、设备制造商"),
    ("食品", "农产品原料供应商、包装材料"),
    ("家电", "面板/压缩机/芯片供应商"),
    ("通信", "光纤光缆、基站设备、芯片供应商"),
    ("计算机", "芯片/存储/服务器供应商"),
];

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    let mut main_business: Vec<Value> = Vec::new();
    let mut breakdown_top: Vec<Value> = Vec::new();
    let mut ths_zyjs = Map::new();

    if ti.market == "A" {
        match fetch_zyjs(&ti.code) {
            Ok(fields) => ths_zyjs = fields,
            Err(e) => {
                ths_zyjs.insert("error".into(), json!(first_n(&e, 80)));
            }
        }

        let sym = format!(
            "{}{}",
            if ti.full.ends_with("SZ") { "SZ" } else { "SH" },
            ti.code
        );
        match fetch_zygc(&sym) {
            Ok(records) if !records.is_empty() => {
                breakdown_top = build_breakdown(&records);
                main_business = records;
            }
            Ok(_) => {}
            Err(e) => {
                main_business = vec![json!({"error": first_n(&e, 200)})];
            }
        }
    }

    // v2.2 · infer 上下游 from 主营业务 + 产品类型 + 经营范围
    let mut upstream = "—".to_string();
    let mut downstream = "—".to_string();
    let mut products = "—".to_string();
    if !ths_zyjs.is_empty() && !ths_zyjs.contains_key("error") {
        let s = |k: &str| -> String {
            ths_zyjs
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let biz = s("主营业务");
        let prod = s("产品类型");
        let scope = s("经营范围");

        if !prod.is_empty() && prod != "nan" && prod != "—" {
            products = first_n(&prod, 100);
        } else if !biz.is_empty() {
            products = first_n(&biz, 100);
        }

        if !biz.is_empty() {
            downstream = first_n(&biz, 80);
        }

        let combined = format!("{biz} {prod} {scope}");
        for (key, val) in UPSTREAM_HINTS {
            if combined.contains(*key) {
                upstream = (*val).to_string();
                break;
            }
        }
        if upstream == "—" && !scope.is_empty() {
            upstream = format!("(从经营范围推断) {}", first_n(&scope, 80));
        }
    }

    let main_business_raw: Vec<Value> = main_business.iter().take(20).cloned().collect();

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "main_business_breakdown": breakdown_top,
            "main_business_raw": main_business_raw,
            "ths_zyjs": Value::Object(ths_zyjs),
            "products": products,
            "upstream": upstream,
            "downstream": downstream,
            "client_concentration": "—",
            "supplier_concentration": "—",
            "_note": "上下游基于主营/产品/经营范围推断，精确数据需年报附注",
        },
        "source": "akshare:stock_zygc_em + stock_zyjs_ths",
        "fallback": false,
    }))
}
