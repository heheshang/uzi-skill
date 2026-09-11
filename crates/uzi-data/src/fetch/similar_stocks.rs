//! Port of `fetch_similar_stocks.py`.

use serde_json::{json, Value};

use uzi_core::py;
use uzi_core::ticker::parse_ticker;

// Industry → peer stock codes (top 4-6 by market cap)
const INDUSTRY_PEERS: &[(&str, &[(&str, &str)])] = &[
    (
        "光学光电子",
        &[
            ("002273", "水晶光电"),
            ("002281", "光迅科技"),
            ("300433", "蓝思科技"),
            ("688127", "蓝特光学"),
            ("002456", "欧菲光"),
            ("603501", "韦尔股份"),
        ],
    ),
    (
        "白酒",
        &[
            ("600519", "贵州茅台"),
            ("000858", "五粮液"),
            ("000568", "泸州老窖"),
            ("002304", "洋河股份"),
            ("600809", "山西汾酒"),
        ],
    ),
    (
        "半导体",
        &[
            ("688981", "中芯国际"),
            ("603986", "兆易创新"),
            ("002371", "北方华创"),
            ("688012", "中微公司"),
            ("688008", "澜起科技"),
            ("002129", "TCL中环"),
        ],
    ),
    (
        "电池",
        &[
            ("300750", "宁德时代"),
            ("300014", "亿纬锂能"),
            ("002460", "赣锋锂业"),
            ("002812", "恩捷股份"),
            ("300207", "欣旺达"),
        ],
    ),
    (
        "医药生物",
        &[
            ("300760", "迈瑞医疗"),
            ("600276", "恒瑞医药"),
            ("603259", "药明康德"),
            ("600196", "复星医药"),
            ("300122", "智飞生物"),
        ],
    ),
    (
        "银行",
        &[
            ("601398", "工商银行"),
            ("600036", "招商银行"),
            ("601939", "建设银行"),
            ("601288", "农业银行"),
            ("601166", "兴业银行"),
        ],
    ),
    (
        "家电",
        &[
            ("000333", "美的集团"),
            ("000651", "格力电器"),
            ("600690", "海尔智家"),
            ("002032", "苏泊尔"),
        ],
    ),
    (
        "光模块",
        &[
            ("300308", "中际旭创"),
            ("300394", "天孚通信"),
            ("300502", "新易盛"),
            ("002463", "沪电股份"),
        ],
    ),
    (
        "消费电子",
        &[
            ("002475", "立讯精密"),
            ("002241", "歌尔股份"),
            ("002938", "鹏鼎控股"),
        ],
    ),
    (
        "钢铁",
        &[
            ("600019", "宝钢股份"),
            ("600808", "马钢股份"),
            ("000898", "鞍钢股份"),
        ],
    ),
    (
        "保险",
        &[
            ("601318", "中国平安"),
            ("601601", "中国太保"),
            ("601628", "中国人寿"),
        ],
    ),
    (
        "证券",
        &[
            ("600030", "中信证券"),
            ("601688", "华泰证券"),
            ("000776", "广发证券"),
        ],
    ),
    (
        "房地产",
        &[
            ("000002", "万科A"),
            ("001979", "招商蛇口"),
            ("600048", "保利发展"),
        ],
    ),
    (
        "食品饮料",
        &[("600887", "伊利股份"), ("603288", "海天味业")],
    ),
    (
        "建筑装饰",
        &[
            ("601668", "中国建筑"),
            ("601186", "中国铁建"),
            ("601390", "中国中铁"),
            ("601800", "中国交建"),
            ("002051", "中工国际"),
        ],
    ),
    (
        "建筑材料",
        &[
            ("600585", "海螺水泥"),
            ("000877", "天山股份"),
            ("002271", "东方雨虹"),
            ("003816", "中南建设"),
        ],
    ),
    (
        "汽车",
        &[
            ("002594", "比亚迪"),
            ("601238", "广汽集团"),
            ("600104", "上汽集团"),
            ("000625", "长安汽车"),
            ("601127", "赛力斯"),
        ],
    ),
    (
        "计算机",
        &[
            ("002230", "科大讯飞"),
            ("000977", "浪潮信息"),
            ("002415", "海康威视"),
            ("688111", "金山办公"),
        ],
    ),
    (
        "通信",
        &[
            ("000063", "中兴通讯"),
            ("600050", "中国联通"),
            ("601728", "中国电信"),
        ],
    ),
    (
        "电力设备",
        &[
            ("601012", "隆基绿能"),
            ("300274", "阳光电源"),
            ("002459", "晶澳科技"),
        ],
    ),
    (
        "煤炭",
        &[
            ("601088", "中国神华"),
            ("600188", "兖矿能源"),
            ("601898", "中煤能源"),
        ],
    ),
    (
        "石油石化",
        &[
            ("600028", "中国石化"),
            ("601857", "中国石油"),
            ("600346", "恒力石化"),
        ],
    ),
    (
        "有色金属",
        &[
            ("601899", "紫金矿业"),
            ("603993", "洛阳钼业"),
            ("002466", "天齐锂业"),
        ],
    ),
    (
        "军工",
        &[
            ("600893", "航发动力"),
            ("000768", "中航飞机"),
            ("601989", "中国重工"),
        ],
    ),
    (
        "量子",
        &[
            ("688027", "国盾量子"),
            ("688599", "天箭科技"),
            ("600770", "综艺股份"),
        ],
    ),
    (
        "港口",
        &[
            ("601018", "宁波港"),
            ("600017", "日照港"),
            ("600018", "上港集团"),
            ("000905", "厦门港务"),
            ("601298", "青岛港"),
            ("000507", "珠海港"),
            ("600190", "锦州港"),
        ],
    ),
    (
        "交通运输",
        &[
            ("601006", "大秦铁路"),
            ("600009", "上海机场"),
            ("601111", "中国国航"),
            ("600029", "南方航空"),
            ("601872", "招商轮船"),
            ("600026", "中远海能"),
        ],
    ),
    (
        "物流",
        &[
            ("002468", "申通快递"),
            ("002352", "顺丰控股"),
            ("600233", "圆通速递"),
            ("002120", "韵达股份"),
            ("603056", "德邦股份"),
        ],
    ),
    (
        "航运",
        &[
            ("601866", "中远海控"),
            ("601872", "招商轮船"),
            ("600026", "中远海能"),
            ("601880", "辽港股份"),
            ("000582", "北部港湾"),
        ],
    ),
    (
        "电力",
        &[
            ("600900", "长江电力"),
            ("601985", "中国核电"),
            ("600886", "国投电力"),
            ("003816", "中国广核"),
            ("600023", "浙能电力"),
        ],
    ),
    (
        "农业",
        &[
            ("000998", "隆平高科"),
            ("002714", "牧原股份"),
            ("300498", "温氏股份"),
            ("600438", "通威股份"),
            ("002311", "海大集团"),
        ],
    ),
    (
        "传媒",
        &[
            ("300027", "华谊兄弟"),
            ("002602", "世纪华通"),
            ("603444", "吉比特"),
            ("300413", "芒果超媒"),
            ("002607", "中公教育"),
        ],
    ),
    (
        "医疗器械",
        &[
            ("300760", "迈瑞医疗"),
            ("688139", "海尔生物"),
            ("300003", "乐普医疗"),
            ("300015", "爱尔眼科"),
            ("688029", "南微医学"),
        ],
    ),
    (
        "环保",
        &[
            ("601200", "上海环境"),
            ("300070", "碧水源"),
            ("603568", "伟明环保"),
            ("000967", "盈峰环境"),
        ],
    ),
];

// v2.2 · 行业别名映射（XueQiu/EastMoney 返回的名称可能不同于 INDUSTRY_PEERS 的 key）
// v3.9.4 · 提升为模块级，供 fetch_peers 的 Tier 3.5 兜底复用（Codex P2）
const INDUSTRY_ALIASES: &[(&str, &str)] = &[
    ("港口航运", "港口"),
    ("港口服务", "港口"),
    ("港口运输", "港口"),
    ("航空运输", "交通运输"),
    ("公路铁路运输", "交通运输"),
    ("铁路运输", "交通运输"),
    ("海运", "航运"),
    ("水上运输", "航运"),
    ("远洋运输", "航运"),
    ("快递物流", "物流"),
    ("仓储物流", "物流"),
    ("火电", "电力"),
    ("水电", "电力"),
    ("核电", "电力"),
    ("新能源发电", "电力"),
    ("种植业", "农业"),
    ("养殖业", "农业"),
    ("饲料", "农业"),
    ("畜禽养殖", "农业"),
    ("游戏", "传媒"),
    ("影视", "传媒"),
    ("广告", "传媒"),
    ("医疗服务", "医疗器械"),
    ("医疗设备", "医疗器械"),
    ("白色家电", "家电"),
    ("小家电", "家电"),
    ("厨卫电器", "家电"),
    ("集成电路", "半导体"),
    ("芯片", "半导体"),
    ("芯片设计", "半导体"),
    ("锂电池", "电池"),
    ("动力电池", "电池"),
    ("储能", "电池"),
    ("光伏设备", "电力设备"),
    ("风电设备", "电力设备"),
    ("白酒", "白酒"),
    ("啤酒", "食品饮料"),
    ("饮料", "食品饮料"),
    ("乳制品", "食品饮料"),
    ("黄金", "有色金属"),
    ("铜", "有色金属"),
    ("铝", "有色金属"),
    ("锂", "有色金属"),
    ("航空发动机", "军工"),
    ("航天", "军工"),
    ("船舶制造", "军工"),
    // v2.8.4 · 申万三级行业 → INDUSTRY_PEERS key 别名映射
    ("工业金属", "有色金属"),
    ("贵金属", "有色金属"),
    ("小金属", "有色金属"),
    ("能源金属", "有色金属"),
    ("稀有金属", "有色金属"),
    ("金属新材料", "有色金属"),
    ("普钢", "钢铁"),
    ("特钢", "钢铁"),
    ("冶钢原料", "钢铁"),
    ("煤炭开采", "煤炭"),
    ("焦炭", "煤炭"),
    ("油气开采", "石油石化"),
    ("炼化及贸易", "石油石化"),
    ("油服工程", "石油石化"),
    ("化学原料", "化工"),
    ("化学制品", "化工"),
    ("化学纤维", "化工"),
    ("塑料", "化工"),
    ("橡胶", "化工"),
    ("农药", "化工"),
    ("农化制品", "化工"),
    ("通用设备", "电力设备"),
    ("专用设备", "电力设备"),
    ("光伏", "电力设备"),
    ("风电", "电力设备"),
    ("电网设备", "电力设备"),
    ("电子化学品", "半导体"),
    ("元件", "半导体"),
    ("光学光电子", "半导体"),
    ("消费电子", "半导体"),
    ("其他电子", "半导体"),
    ("乘用车", "汽车"),
    ("商用车", "汽车"),
    ("汽车零部件", "汽车"),
    ("化学制药", "医药生物"),
    ("中药", "医药生物"),
    ("生物制品", "医药生物"),
];

fn peers_for(industry: &str) -> Option<&'static [(&'static str, &'static str)]> {
    INDUSTRY_PEERS
        .iter()
        .find(|(k, _)| *k == industry)
        .map(|(_, v)| *v)
}

fn alias_for(industry: &str) -> Option<&'static str> {
    INDUSTRY_ALIASES
        .iter()
        .find(|(k, _)| *k == industry)
        .map(|(_, v)| *v)
}

/// Python f-string interpolation of a value: `str(v)`, with JSON numbers
/// rendered by [`py::num_str`] so whole-valued floats keep their `.0`.
fn fstr(v: &Value) -> String {
    match v {
        Value::Number(_) => py::num_str(v),
        other => py::py_display(other),
    }
}

/// `similarity_score = int(max(75, min(98, pe_sim if pe_sim > 0 else 85)))`.
fn similarity_score(self_pe: f64, peer_pe: f64) -> i64 {
    let mut pe_sim = 0.0f64;
    if self_pe != 0.0 && peer_pe != 0.0 {
        pe_sim = self_pe.min(peer_pe) / self_pe.max(peer_pe) * 100.0;
    }
    let base = if pe_sim > 0.0 { pe_sim } else { 85.0 };
    base.min(98.0).max(75.0) as i64
}

fn fetch_peer_basics(
    peers: &[(&str, &str)],
    self_code: &str,
    top_n: usize,
) -> Vec<Value> {
    let mut results = Vec::new();
    for &(code, known_name) in peers {
        if code == self_code {
            continue;
        }
        if results.len() >= top_n {
            break;
        }
        let ti = parse_ticker(code);
        let basic = crate::sources::fetch_basic(&ti);
        if !py::truthy(&basic) || !py::truthy(basic.get("price").unwrap_or(&Value::Null)) {
            continue;
        }
        let name = basic
            .get("name")
            .filter(|v| py::truthy(v))
            .cloned()
            .unwrap_or_else(|| json!(known_name));
        let url = if ti.full.ends_with("SZ") {
            format!("https://xueqiu.com/S/SZ{code}")
        } else {
            format!("https://xueqiu.com/S/SH{code}")
        };
        results.push(json!({
            "name": name,
            "code": ti.full,
            "price": basic.get("price").cloned().unwrap_or(Value::Null),
            "pe_ttm": basic.get("pe_ttm").cloned().unwrap_or(Value::Null),
            "pb": basic.get("pb").cloned().unwrap_or(Value::Null),
            "market_cap": basic.get("market_cap").cloned().unwrap_or(Value::Null),
            "change_pct": basic.get("change_pct").cloned().unwrap_or(Value::Null),
            "url": url,
        }));
    }
    results
}

pub fn main(ticker: &str) -> Result<Value, String> {
    const TOP_N: usize = 4;
    let ti = parse_ticker(ticker);
    if ti.market != "A" {
        return Ok(json!({
            "ticker": ti.full,
            "data": {"similar_stocks": []},
            "source": "n/a",
            "fallback": true,
        }));
    }

    let basic = crate::sources::fetch_basic(&ti);
    let industry = basic
        .get("industry")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Guard: industry must be a non-empty string for matching
    if industry.trim().chars().count() < 2 {
        let display = if industry.is_empty() {
            "未知".to_string()
        } else {
            industry.clone()
        };
        return Ok(json!({
            "ticker": ti.full,
            "data": {
                "similar_stocks": [],
                "industry": display,
                "_note": "行业未识别，无法匹配同行",
            },
            "source": "INDUSTRY_PEERS (no industry)",
            "fallback": true,
        }));
    }

    // 1. 精确匹配
    let mut peers = peers_for(&industry);
    // 2. 别名映射
    if peers.is_none() {
        if let Some(alias) = alias_for(&industry) {
            peers = peers_for(alias);
        }
    }
    // 3. 子串模糊匹配
    if peers.is_none() {
        let prefix: String = industry.chars().take(2).collect();
        for (key, val) in INDUSTRY_PEERS {
            if key.contains(industry.as_str())
                || industry.contains(*key)
                || key.contains(prefix.as_str())
            {
                peers = Some(*val);
                break;
            }
        }
    }

    let Some(peers) = peers else {
        return Ok(json!({
            "ticker": ti.full,
            "data": {
                "similar_stocks": [],
                "industry": industry,
                "_note": format!("行业 '{industry}' 未在同行映射表里"),
            },
            "source": "INDUSTRY_PEERS (missing)",
            "fallback": true,
        }));
    };

    let peer_basics = fetch_peer_basics(peers, &ti.code, TOP_N);

    // Build similar_stocks output with similarity score + reason
    let mut similar = Vec::new();
    let self_pe = py::f(basic.get("pe_ttm").unwrap_or(&Value::Null), 0.0);
    for p in &peer_basics {
        let p_pe = py::f(p.get("pe_ttm").unwrap_or(&Value::Null), 0.0);
        let score = similarity_score(self_pe, p_pe);

        similar.push(json!({
            "name": p.get("name").cloned().unwrap_or(Value::Null),
            "code": p.get("code").cloned().unwrap_or(Value::Null),
            "price": p.get("price").cloned().unwrap_or(Value::Null),
            "pe_ttm": p.get("pe_ttm").cloned().unwrap_or(Value::Null),
            "market_cap": p.get("market_cap").cloned().unwrap_or(Value::Null),
            "change_pct": p.get("change_pct").cloned().unwrap_or(Value::Null),
            "similarity": format!("{score}%"),
            "reason": format!(
                "同属{industry} · PE {} · 市值 {}",
                fstr(p.get("pe_ttm").unwrap_or(&Value::Null)),
                fstr(p.get("market_cap").unwrap_or(&Value::Null)),
            ),
            "url": p.get("url").cloned().unwrap_or(Value::Null),
        }));
    }

    Ok(json!({
        "ticker": ti.full,
        "data": {
            "similar_stocks": similar,
            "industry": industry,
            "peers_attempted": peers.len(),
        },
        "source": "INDUSTRY_PEERS + fetch_basic (XueQiu / baidu / sina)",
        "fallback": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::similarity_score;

    #[test]
    fn similarity_score_clamps_match_upstream() {
        assert_eq!(similarity_score(0.0, 10.0), 85);
        assert_eq!(similarity_score(10.0, 0.0), 85);
        assert_eq!(similarity_score(10.0, 10.0), 98);
        assert_eq!(similarity_score(10.0, 12.0), 83);
        assert_eq!(similarity_score(1.0, 100.0), 75);
    }
}
