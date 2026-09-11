//! Port of `lib/industry_mapping.py` — 申万三级行业 → 证监会行业分类 语义映射.
//!
//! cninfo's `stock_industry_pe_ratio_cninfo` returns 证监会 industry names while
//! `fetch_basic` produces 申万 names; the two taxonomies do not overlap
//! literally, so a hard map plus a four-stage resolver replaces the old
//! `str.contains(industry[:2])` fuzzy match that misclassified 工业金属 as
//! 农副食品加工业.

use serde_json::Value;

/// `SW_TO_CSRC_INDUSTRY` — 137 申万三级行业 entries.
pub static SW_TO_CSRC_INDUSTRY: &[(&str, &str)] = &[
    ("工业金属", "有色金属冶炼和压延加工业"),
    ("金属新材料", "有色金属冶炼和压延加工业"),
    ("小金属", "有色金属冶炼和压延加工业"),
    ("贵金属", "有色金属冶炼和压延加工业"),
    ("能源金属", "有色金属冶炼和压延加工业"),
    ("稀有金属", "有色金属冶炼和压延加工业"),
    ("有色金属", "有色金属冶炼和压延加工业"),
    ("普钢", "黑色金属冶炼和压延加工业"),
    ("特钢", "黑色金属冶炼和压延加工业"),
    ("冶钢原料", "黑色金属矿采选业"),
    ("钢铁", "黑色金属冶炼和压延加工业"),
    ("煤炭开采", "煤炭开采和洗选业"),
    ("焦炭", "石油、煤炭及其他燃料加工业"),
    ("油气开采", "石油和天然气开采业"),
    ("油服工程", "开采专业及辅助性活动"),
    ("炼化及贸易", "石油、煤炭及其他燃料加工业"),
    ("通用设备", "通用设备制造业"),
    ("专用设备", "专用设备制造业"),
    ("工程机械", "专用设备制造业"),
    ("轨交设备", "铁路、船舶、航空航天和其他运输设备制造业"),
    ("船舶制造", "铁路、船舶、航空航天和其他运输设备制造业"),
    ("航空装备", "铁路、船舶、航空航天和其他运输设备制造业"),
    ("地面兵装", "铁路、船舶、航空航天和其他运输设备制造业"),
    ("航天装备", "铁路、船舶、航空航天和其他运输设备制造业"),
    ("工业母机", "专用设备制造业"),
    ("机器人", "专用设备制造业"),
    ("机械制造", "通用设备制造业"),
    ("工业机械", "通用设备制造业"),
    ("磨具磨料", "专用设备制造业"),
    ("能源及重型设备", "专用设备制造业"),
    ("化学原料", "化学原料和化学制品制造业"),
    ("化学制品", "化学原料和化学制品制造业"),
    ("化学纤维", "化学纤维制造业"),
    ("塑料", "化学原料和化学制品制造业"),
    ("橡胶", "化学原料和化学制品制造业"),
    ("农化制品", "化学原料和化学制品制造业"),
    ("农药", "化学原料和化学制品制造业"),
    ("非金属材料", "非金属矿物制品业"),
    ("基础化工", "化学原料和化学制品制造业"),
    ("白酒", "酒、饮料和精制茶制造业"),
    ("葡萄酒", "酒、饮料和精制茶制造业"),
    ("啤酒", "酒、饮料和精制茶制造业"),
    ("其他酒类", "酒、饮料和精制茶制造业"),
    ("非白酒", "酒、饮料和精制茶制造业"),
    ("软饮料", "酒、饮料和精制茶制造业"),
    ("乳品", "食品制造业"),
    ("食品加工", "食品制造业"),
    ("调味发酵品", "食品制造业"),
    ("休闲食品", "食品制造业"),
    ("肉制品", "农副食品加工业"),
    ("粮油加工", "农副食品加工业"),
    ("饲料", "农副食品加工业"),
    ("零食", "食品制造业"),
    ("化学制药", "医药制造业"),
    ("中药", "医药制造业"),
    ("生物制品", "医药制造业"),
    ("医疗器械", "专用设备制造业"),
    ("医疗服务", "卫生"),
    ("医药商业", "批发业"),
    ("医药生物", "医药制造业"),
    ("半导体", "计算机、通信和其他电子设备制造业"),
    ("电子化学品", "化学原料和化学制品制造业"),
    ("元件", "计算机、通信和其他电子设备制造业"),
    ("光学光电子", "计算机、通信和其他电子设备制造业"),
    ("消费电子", "计算机、通信和其他电子设备制造业"),
    ("其他电子", "计算机、通信和其他电子设备制造业"),
    ("电池", "电气机械和器材制造业"),
    ("光伏设备", "电气机械和器材制造业"),
    ("风电设备", "电气机械和器材制造业"),
    ("电网设备", "电气机械和器材制造业"),
    ("电机", "电气机械和器材制造业"),
    ("其他电源设备", "电气机械和器材制造业"),
    ("乘用车", "汽车制造业"),
    ("商用车", "汽车制造业"),
    ("摩托车及其他", "汽车制造业"),
    ("汽车零部件", "汽车制造业"),
    ("汽车服务", "批发业"),
    ("国有大型银行", "货币金融服务"),
    ("股份制银行", "货币金融服务"),
    ("城商行", "货币金融服务"),
    ("农商行", "货币金融服务"),
    ("银行", "货币金融服务"),
    ("证券", "资本市场服务"),
    ("保险", "保险业"),
    ("多元金融", "资本市场服务"),
    ("房地产开发", "房地产业"),
    ("房地产服务", "房地产业"),
    ("物业管理", "商务服务业"),
    ("计算机设备", "计算机、通信和其他电子设备制造业"),
    ("软件开发", "软件和信息技术服务业"),
    ("IT服务", "软件和信息技术服务业"),
    ("通信设备", "计算机、通信和其他电子设备制造业"),
    ("通信服务", "电信、广播电视和卫星传输服务"),
    ("游戏", "互联网和相关服务"),
    ("互联网电商", "互联网和相关服务"),
    ("广告营销", "广播、电视、电影和录音制作业"),
    ("影视院线", "广播、电视、电影和录音制作业"),
    ("出版", "新闻和出版业"),
    ("文化传媒", "新闻和出版业"),
    ("火电", "电力、热力生产和供应业"),
    ("水电", "电力、热力生产和供应业"),
    ("核电", "电力、热力生产和供应业"),
    ("新能源发电", "电力、热力生产和供应业"),
    ("热力服务", "电力、热力生产和供应业"),
    ("燃气", "燃气生产和供应业"),
    ("水务", "水的生产和供应业"),
    ("电力", "电力、热力生产和供应业"),
    ("种植业", "农、林、牧、渔专业及辅助性活动"),
    ("林业", "林业"),
    ("养殖业", "畜牧业"),
    ("渔业", "渔业"),
    ("农产品加工", "农副食品加工业"),
    ("航运港口", "水上运输业"),
    ("航空机场", "航空运输业"),
    ("铁路公路", "铁路运输业"),
    ("物流", "装卸搬运和仓储业"),
    ("快递", "邮政业"),
    ("家具用品", "家具制造业"),
    ("家居用品", "家具制造业"),
    ("造纸", "造纸和纸制品业"),
    ("包装印刷", "印刷和记录媒介复制业"),
    ("纺织制造", "纺织业"),
    ("服装家纺", "纺织服装、服饰业"),
    ("饰品", "文教、工美、体育和娱乐用品制造业"),
    ("一般零售", "零售业"),
    ("专业连锁", "零售业"),
    ("贸易", "批发业"),
    ("跨境电商", "互联网和相关服务"),
    ("房屋建设", "房屋建筑业"),
    ("基建建设", "土木工程建筑业"),
    ("装修建设", "建筑装饰、装修和其他建筑业"),
    ("水泥", "非金属矿物制品业"),
    ("玻璃玻纤", "非金属矿物制品业"),
    ("酒店餐饮", "住宿和餐饮业"),
    ("旅游及景区", "文化艺术业"),
    ("教育", "教育"),
    ("专业服务", "商务服务业"),
];

/// Tokens shared by many 证监会 names; using them as a `contains` prefix
/// mis-matches (工业 appears in four industries, 制造 in 30+).
pub const HIGH_COLLISION_TOKENS: &[&str] = &[
    "工业", "加工", "制造", "服务", "生产", "供应", "设备", "制品", "其他", "专业", "业", "业务",
];

/// `SW_TO_CSRC_INDUSTRY.get(sw_industry)`.
pub fn sw_to_csrc(sw_industry: &str) -> Option<&'static str> {
    SW_TO_CSRC_INDUSTRY
        .iter()
        .find(|(k, _)| *k == sw_industry)
        .map(|(_, v)| *v)
}

/// `_first_meaningful_prefix(s, n)` — first n non-blacklisted chars.
pub fn first_meaningful_prefix(s: &str, n: usize) -> String {
    if s.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let prefix: String = chars.iter().take(n).collect();
    if HIGH_COLLISION_TOKENS.contains(&prefix.as_str()) {
        if chars.len() >= n + 2 {
            return chars[n..n + 2].iter().collect();
        }
        return String::new();
    }
    prefix
}

/// `resolve_csrc_industry(sw_industry, df)`.
///
/// `df` is the cninfo industry table as a JSON array of rows; each row is an
/// object with a `行业名称` key. Returns the matched row (a JSON object) or
/// `None` — never blindly `iloc[0]`.
pub fn resolve_csrc_industry(sw_industry: &str, df: &Value) -> Option<Value> {
    let rows = df.as_array()?;
    if rows.is_empty() || sw_industry.is_empty() {
        return None;
    }
    let name_of = |row: &Value| -> String {
        row.get("行业名称")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    if rows.iter().all(|r| r.get("行业名称").is_none()) {
        return None;
    }

    // ── 策略 1: 硬映射 ──
    if let Some(target) = sw_to_csrc(sw_industry) {
        if let Some(row) = rows.iter().find(|r| name_of(r) == target) {
            return Some(row.clone());
        }
        let head: String = target.chars().take(4).collect();
        if let Some(row) = rows.iter().find(|r| name_of(r).contains(&head)) {
            return Some(row.clone());
        }
    }

    // ── 策略 2: 申万名整体作为子串 ──
    if let Some(row) = rows.iter().find(|r| name_of(r).contains(sw_industry)) {
        return Some(row.clone());
    }

    // ── 策略 3: 去掉高碰撞前缀后 fuzzy ──
    let safe_prefix = first_meaningful_prefix(sw_industry, 2);
    if !safe_prefix.is_empty() && !HIGH_COLLISION_TOKENS.contains(&safe_prefix.as_str()) {
        if let Some(row) = rows.iter().find(|r| name_of(r).contains(&safe_prefix)) {
            return Some(row.clone());
        }
    }

    // ── 策略 4: 全挂 —— 返 None ──
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table() -> Value {
        json!([
            {"行业名称": "农副食品加工业", "pe": 31.2},
            {"行业名称": "石油、煤炭及其他燃料加工业", "pe": 12.0},
            {"行业名称": "黑色金属冶炼和压延加工业", "pe": 15.0},
            {"行业名称": "有色金属冶炼和压延加工业", "pe": 20.0},
        ])
    }

    #[test]
    fn hard_map_beats_prefix_collision() {
        // the v2.8.3 bug: 工业金属 must not land on 农副食品加工业
        let row = resolve_csrc_industry("工业金属", &table()).unwrap();
        assert_eq!(row["行业名称"], json!("有色金属冶炼和压延加工业"));
    }

    #[test]
    fn unmapped_industry_returns_none_not_first_row() {
        assert!(resolve_csrc_industry("完全未知行业XYZ", &table()).is_none());
        assert!(resolve_csrc_industry("", &table()).is_none());
        assert!(resolve_csrc_industry("工业金属", &json!([])).is_none());
    }

    #[test]
    fn substring_strategy_matches_by_full_name() {
        // 有色金属 is in the hard map, but "燃料加工" is not — falls to substring
        let row = resolve_csrc_industry("燃料加工", &table()).unwrap();
        assert_eq!(row["行业名称"], json!("石油、煤炭及其他燃料加工业"));
    }

    #[test]
    fn first_meaningful_prefix_skips_collision_prefixes() {
        assert_eq!(first_meaningful_prefix("工业金属", 2), "金属");
        assert_eq!(first_meaningful_prefix("工业", 2), "");
        assert_eq!(first_meaningful_prefix("白酒", 2), "白酒");
    }

    #[test]
    fn table_covers_the_documented_cases() {
        for sw in ["工业金属", "白酒", "光学光电子", "半导体", "钢铁"] {
            assert!(sw_to_csrc(sw).is_some(), "missing {sw}");
        }
        assert_eq!(sw_to_csrc("完全未知行业XYZ"), None);
    }
}
