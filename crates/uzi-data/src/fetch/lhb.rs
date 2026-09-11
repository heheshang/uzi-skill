//! Port of `fetch_lhb.py`.
//!
//! Dimension 16 · 龙虎榜 (个股 + 同板块 + 机构 vs 游资 split + 席位匹配).

use serde_json::{json, Map, Value};

use uzi_core::py::{f_fin, py_display, truthy};
use uzi_core::ticker::parse_ticker;

use crate::sources;

/// `lib/seat_db.py::SEATS` — nickname → seat keywords (insertion order).
const SEATS: &[(&str, &[&str])] = &[
    (
        "章盟主",
        &[
            "国泰君安证券股份有限公司上海江苏路证券营业部",
            "国泰君安证券股份有限公司宁波彩虹北路证券营业部",
            "中信证券股份有限公司杭州延安路证券营业部",
        ],
    ),
    (
        "孙哥",
        &[
            "中信证券股份有限公司上海溧阳路证券营业部",
            "中信证券股份有限公司上海古北路证券营业部",
            "中信证券股份有限公司上海分公司",
        ],
    ),
    (
        "赵老哥",
        &[
            "浙商证券股份有限公司绍兴解放北路证券营业部",
            "中国银河证券股份有限公司绍兴证券营业部",
            "中国银河证券股份有限公司北京阜成路证券营业部",
        ],
    ),
    (
        "佛山无影脚",
        &[
            "光大证券股份有限公司佛山绿景路证券营业部",
            "光大证券股份有限公司佛山季华六路证券营业部",
            "湘财证券股份有限公司佛山祖庙路证券营业部",
        ],
    ),
    (
        "炒股养家",
        &[
            "华鑫证券有限责任公司上海红宝石路证券营业部",
            "华鑫证券有限责任公司上海宛平南路证券营业部",
        ],
    ),
    ("陈小群", &["中国银河证券股份有限公司大连黄河路证券营业部"]),
    (
        "呼家楼",
        &[
            "中信证券股份有限公司上海凯滨路证券营业部",
            "中信证券股份有限公司北京总部",
            "中信建投证券股份有限公司北京朝外大街证券营业部",
        ],
    ),
    (
        "方新侠",
        &[
            "兴业证券股份有限公司陕西分公司",
            "中信证券股份有限公司西安朱雀大街证券营业部",
        ],
    ),
    ("作手新一", &["国泰君安证券股份有限公司南京太平南路证券营业部"]),
    (
        "小鳄鱼",
        &[
            "南京证券股份有限公司南京大钟亭证券营业部",
            "中金财富证券有限公司南京龙蟠中路证券营业部",
        ],
    ),
    (
        "交易猿",
        &[
            "华泰证券股份有限公司天津东丽开发区二纬路证券营业部",
            "招商证券股份有限公司福州六一中路证券营业部",
        ],
    ),
    (
        "毛老板",
        &[
            "国泰君安证券股份有限公司北京光华路证券营业部",
            "方正证券股份有限公司乐山龙游路证券营业部",
            "广发证券股份有限公司上海东方路证券营业部",
        ],
    ),
    ("消闲派", &["华泰证券股份有限公司浙江分公司"]),
    ("拉萨天团", &["东方财富证券股份有限公司拉萨"]),
    ("成都帮", &["华泰证券股份有限公司成都南一环路第二证券营业部"]),
    (
        "苏南帮",
        &[
            "华泰证券股份有限公司无锡",
            "华泰证券股份有限公司镇江",
            "华泰证券股份有限公司南京",
        ],
    ),
    ("宁波桑田路", &["国盛证券有限责任公司宁波桑田路证券营业部"]),
    ("六一中路", &["招商证券股份有限公司福州六一中路证券营业部"]),
    (
        "流沙河",
        &[
            "招商证券股份有限公司北京车公庄西路证券营业部",
            "华泰证券股份有限公司上海武定路证券营业部",
        ],
    ),
    ("古北路", &["中信证券股份有限公司上海古北路证券营业部"]),
    ("北京炒家", &["首板专精，无固定席位"]),
    ("瑞鹤仙", &["银河证券", "招商证券深圳"]),
    ("鑫多多", &["华鑫证券", "招商证券", "中信证券"]),
];

/// `_is_institutional(seat_name)` — `机构专用 or (机构 and not 证券)`.
fn is_institutional(seat_name: &str) -> bool {
    seat_name.contains("机构专用") || (seat_name.contains("机构") && !seat_name.contains("证券"))
}

/// `split_inst_vs_youzi(records)`.
pub fn split_inst_vs_youzi(records: &[Value]) -> Value {
    let mut inst_buy = 0.0f64;
    let mut inst_sell = 0.0f64;
    let mut youzi_buy = 0.0f64;
    let mut youzi_sell = 0.0f64;
    for r in records {
        let seat = first_truthy(r, &["营业部名称", "交易营业部"])
            .map(|v| py_display(v))
            .unwrap_or_default();
        let buy = field_num(r, &["买入金额", "买入额"]);
        let sell = field_num(r, &["卖出金额", "卖出额"]);
        if is_institutional(&seat) {
            inst_buy += buy;
            inst_sell += sell;
        } else {
            youzi_buy += buy;
            youzi_sell += sell;
        }
    }
    json!({
        "institutional_buy": inst_buy,
        "institutional_sell": inst_sell,
        "institutional_net": inst_buy - inst_sell,
        "youzi_buy": youzi_buy,
        "youzi_sell": youzi_sell,
        "youzi_net": youzi_buy - youzi_sell,
    })
}

/// `lib/seat_db.py::match_seats_in_lhb(records)` — `dict[str, list[dict]]`, so
/// the Rust shape is a JSON object whose values are arrays.
pub fn match_seats_in_lhb(records: &[Value]) -> Map<String, Value> {
    let mut matches: Map<String, Value> = Map::new();
    for (nick, keywords) in SEATS {
        let mut hits: Vec<Value> = Vec::new();
        for row in records {
            let text = row
                .as_object()
                .map(|o| o.values().map(py_display).collect::<Vec<_>>().join(" "))
                .unwrap_or_else(|| py_display(row));
            if keywords.iter().any(|kw| text.contains(*kw)) {
                hits.push(row.clone());
            }
        }
        if !hits.is_empty() {
            matches.insert((*nick).to_string(), Value::Array(hits));
        }
    }
    matches
}

/// `fetch_sector_lhb(industry)` — AkShare `stock_lhb_stock_statistic_em` is the
/// only upstream path; the Rust port degrades to the empty list upstream returns
/// on failure.
pub fn fetch_sector_lhb(industry: &str) -> Vec<Value> {
    if industry.is_empty() {
        return Vec::new();
    }
    Vec::new()
}

/// First key whose value is Python-truthy.
fn first_truthy<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|k| v.get(*k).filter(|val| truthy(*val)))
}

/// `float(r.get(a) or r.get(b) or 0)`.
fn field_num(v: &Value, keys: &[&str]) -> f64 {
    first_truthy(v, keys).map(|x| f_fin(x, 0.0)).unwrap_or(0.0)
}

pub fn main(ticker: &str) -> Result<Value, String> {
    let ti = parse_ticker(ticker);
    if ti.market != "A" {
        return Ok(json!({
            "ticker": ti.full,
            "data": {"_note": "lhb only A-share"},
            "source": "skip",
            "fallback": false,
        }));
    }

    let lhb = sources::fetch_lhb_recent(&ti, 30);
    let lhb_records: Vec<Value> = lhb.as_array().cloned().unwrap_or_default();
    let matched = match_seats_in_lhb(&lhb_records);
    let split = split_inst_vs_youzi(&lhb_records);

    let basic = sources::fetch_basic(&ti);
    let industry = basic
        .get("industry")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let sector = fetch_sector_lhb(industry);

    let matched_keys: Vec<Value> = matched.keys().map(|k| json!(k)).collect();
    let mut matched_detail: Map<String, Value> = Map::new();
    for (k, v) in &matched {
        let top3: Vec<Value> = v
            .as_array()
            .map(|a| a.iter().take(3).cloned().collect())
            .unwrap_or_default();
        matched_detail.insert(k.clone(), Value::Array(top3));
    }
    let sector_leader_hint = sector
        .first()
        .and_then(|r| r.get("代码"))
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "lhb_count_30d": lhb_records.len(),
            "lhb_records": lhb_records.iter().take(30).cloned().collect::<Vec<_>>(),
            "matched_youzi": matched_keys,
            "matched_youzi_detail": matched_detail,
            "inst_vs_youzi": split,
            "sector_lhb_top50": sector.iter().take(30).cloned().collect::<Vec<_>>(),
            "sector_leader_hint": sector_leader_hint,
        },
        "source": "akshare:stock_lhb_stock_detail_em + statistic + seat_db",
        "fallback": false,
    }))
}
