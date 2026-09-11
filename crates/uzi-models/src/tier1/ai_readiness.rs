//! Port of `lib/tier1/ai_readiness.py` — single-stock AI readiness / positioning
//! assessment, adapted from the PE `ai-readiness` skill.

use crate::{dim_data, get_or, pnum, py_list_str_repr, py_str_py};
use serde_json::{json, Value};

use crate::clock;

/// Keyword dictionary: infer AI leverage-point categories from text.
const LEVERAGE_PATTERNS: &[(&str, &[&str])] = &[
    (
        "算力 / AI 芯片",
        &[
            "算力", "ai 芯片", "ai芯片", "asic", "gpu", "risc-v", "交换机", "ai server",
            "ai 服务器", "数据中心", "data center",
        ],
    ),
    (
        "光互连 / 光模块",
        &[
            "光模块", "光芯片", "cpo", "光引擎", "硅光", "光通信", "光器件", "激光器", "eml",
            "vcsel", "inp", "磷化铟", "光纤", "空芯光纤",
        ],
    ),
    (
        "存储 / HBM",
        &["hbm", "存储", "ddr", "封装基板", "abf", "载板", "cowos", "先进封装"],
    ),
    ("供电 / 散热", &["液冷", "散热", "电源", "bbu", "服务器电源", "pdu"]),
    (
        "互连 / 连接器 / PCB",
        &["pcb", "高速铜", "铜连接", "铜缆", "背板连接器", "连接器"],
    ),
    (
        "AI 终端光学 (AR/VR)",
        &[
            "光波导", "衍射光波导", "waveguide", "micro-led", "microled", "硅基oled",
            "近眼显示", "头显", "增强现实", "虚拟现实", "ar 眼镜", "ar眼镜", "车载光学",
            "晶圆级光学", "镜头", "摄像模组",
        ],
    ),
    (
        "AI 应用 / 软件",
        &[
            "大模型", "生成式", "aigc", "智能体", "agent", "ai 应用", "推理", "训练", "算法",
            "ai saas",
        ],
    ),
    (
        "AI 赋能传统业务",
        &["智能化", "数字化", "降本增效", "自动化", "机器视觉", "智能制造"],
    ),
];

/// `_infer_leverage_points` — top AI leverage points (max 3).
fn infer_leverage_points(blob: &str, _ai_keywords: &[String]) -> Vec<Value> {
    let blob = blob.to_lowercase();
    let mut points: Vec<Value> = Vec::new();
    for (category, kws) in LEVERAGE_PATTERNS {
        let hits: Vec<String> = kws
            .iter()
            .filter(|kw| blob.contains(**kw))
            .map(|kw| (*kw).to_string())
            .collect();
        if hits.is_empty() {
            continue;
        }
        let stance = if *category == "AI 应用 / 软件" || *category == "AI 赋能传统业务" {
            "赋能 — AI 提升自身业务效率/产品力"
        } else {
            "卡位 — 处于 AI 算力/终端供应链关键环节"
        };
        points.push(json!({
            "category": category,
            "keywords": hits.into_iter().take(4).collect::<Vec<_>>(),
            "stance": stance,
        }));
    }
    points.truncate(3);
    points
}

/// `build_ai_readiness` — single-stock AI readiness / positioning assessment.
pub fn build_ai_readiness(features: &Value, raw_data: &Value) -> Value {
    let chain = dim_data(raw_data, "5_chain");
    let events = dim_data(raw_data, "15_events");
    let industry = dim_data(raw_data, "7_industry");

    let name = get_or(features, "name", json!("—"));
    let code = {
        let c = uzi_core::py::get(features, "code");
        if uzi_core::py::truthy(c) {
            c.clone()
        } else {
            get_or(features, "ticker", json!("—"))
        }
    };

    // Reuse the Serenity derived features as the positioning anchor.
    let choke = pnum(uzi_core::py::get(features, "ai_chokepoint_score"), 0.0);
    let ai_chain_hit = uzi_core::py::truthy(uzi_core::py::get(features, "ai_chain_hit"));
    let ai_keywords: Vec<String> = uzi_core::py::get(features, "ai_chain_keywords")
        .as_array()
        .map(|a| a.iter().map(py_str_py).collect())
        .unwrap_or_default();
    let ai_irreplaceable = uzi_core::py::truthy(uzi_core::py::get(features, "ai_irreplaceable"));
    let ai_smallcap = uzi_core::py::truthy(uzi_core::py::get(features, "ai_smallcap"));
    let moat_total = pnum(uzi_core::py::get(features, "moat_total"), 0.0);

    // Gate ②: verifiable real AI revenue / orders / capacity.
    let chain_txt = if uzi_core::py::truthy(chain) {
        uzi_core::json::to_py_compact(chain)
    } else {
        String::new()
    };
    let timeline = get_or(events, "event_timeline", json!([]));
    let events_txt = match timeline.as_array() {
        Some(a) => a.iter().map(py_str_py).collect::<Vec<_>>().join(" "),
        None => py_str_py(&timeline),
    };
    let industry_growth = {
        let g = pnum(uzi_core::py::get(features, "industry_growth"), 0.0);
        if g != 0.0 {
            g
        } else {
            pnum(uzi_core::py::get(industry, "growth"), 0.0)
        }
    };

    let evidence_blob = format!("{} {}", chain_txt, events_txt).to_lowercase();
    const REVENUE_EVIDENCE_KW: &[&str] = &[
        "订单", "中标", "签约", "长协", "在手", "backlog", "供货", "导入", "量产", "扩产",
        "产能", "predict", "放量", "提价", "缺货", "满产", "认证", "送样",
    ];
    let evidence_hits: Vec<String> = REVENUE_EVIDENCE_KW
        .iter()
        .filter(|kw| evidence_blob.contains(**kw))
        .map(|kw| (*kw).to_string())
        .collect();
    let has_real_demand = ai_chain_hit && (!evidence_hits.is_empty() || industry_growth >= 20.0);

    let gate2_basis = {
        let mut s = String::new();
        if !evidence_hits.is_empty() {
            s.push_str(&format!(
                "证据词 {}",
                py_list_str_repr(&evidence_hits[..evidence_hits.len().min(4)])
            ));
        }
        if industry_growth >= 20.0 {
            s.push_str(&format!(" · 行业增速 {:.0}%", industry_growth));
        }
        let trimmed = s.trim_matches(|c| c == ' ' || c == '·').to_string();
        if trimmed.is_empty() {
            "缺订单/产能/景气证据，疑似纯概念标签".to_string()
        } else {
            trimmed
        }
    };

    let gates = json!([
        {
            "gate": "① 是否真在 AI 产业链上",
            "pass": ai_chain_hit,
            "basis": if ai_chain_hit {
                format!("命中 AI 链关键词 {}", py_list_str_repr(&ai_keywords[..ai_keywords.len().min(5)]))
            } else {
                "未在 AI 算力/终端供应链上检出关键词".to_string()
            },
        },
        {
            "gate": "② 是否有可验证的 AI 真实收入/订单/产能",
            "pass": has_real_demand,
            "basis": gate2_basis,
        },
        {
            "gate": "③ 卡位是否不可替代且可持续",
            "pass": ai_irreplaceable,
            "basis": if ai_irreplaceable {
                format!("切换成本+规模壁垒达标（moat {:.0}/40）", moat_total)
            } else {
                format!("不可替代性不足（moat {:.0}/40，切换+规模未达 12/20）", moat_total)
            },
        },
    ]);
    let passed = gates
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| uzi_core::py::truthy(uzi_core::py::get(g, "pass")))
        .count();
    let all_pass = passed == 3;

    // AI exposure rating (strong / medium / weak / none).
    let (rating, rating_note) = if !ai_chain_hit {
        (
            "无",
            "不在 AI 产业链上 — AI 浪潮对其基本面无直接传导",
        )
    } else if choke >= 75.0 || (all_pass && choke >= 60.0) {
        ("强", "AI 卡位硬 + 真实需求可验证 + 不可替代 → 强就绪")
    } else if choke >= 50.0 || passed >= 2 {
        ("中", "在 AI 链上且部分 gate 成立，卡位待验证")
    } else {
        ("弱", "AI 暴露偏弱 / 仅蹭概念，卡位证据不足")
    };

    // Gaps (failed gates).
    let gaps: Vec<Value> = gates
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| !uzi_core::py::truthy(uzi_core::py::get(g, "pass")))
        .map(|g| uzi_core::py::get(g, "gate").clone())
        .collect();

    // Go / Wait verdict.
    let (verdict, verdict_note) = if all_pass {
        (
            "Go · 强就绪",
            "三道 gate 全通过 — AI 卡位可作为核心投资逻辑之一".to_string(),
        )
    } else {
        let note = if !gaps.is_empty() {
            format!(
                "缺口：{}",
                gaps.iter().map(py_str_py).collect::<Vec<_>>().join("；")
            )
        } else {
            "待补充验证".to_string()
        };
        ("Wait · 观察", note)
    };

    // Top 2-3 AI leverage points.
    let ind_txt = if industry.is_object() {
        uzi_core::json::to_py_compact(industry)
    } else {
        py_str_py(industry)
    };
    let leverage_blob = [
        py_str_py(&get_or(features, "industry", json!(""))),
        py_str_py(&name),
        chain_txt.clone(),
        ind_txt,
        events_txt,
        ai_keywords.join(" "),
    ]
    .join(" ");
    let mut leverage_points = infer_leverage_points(&leverage_blob, &ai_keywords);
    if leverage_points.is_empty() && ai_chain_hit {
        leverage_points = vec![json!({
            "category": "AI 产业链关联（未归类）",
            "keywords": ai_keywords.iter().take(4).cloned().collect::<Vec<_>>(),
            "stance": "卡位 — 已检出 AI 链关键词，建议人工细分环节",
        })];
    }

    // One-line conclusion.
    let conclusion = if rating == "无" {
        format!(
            "{}（{}）不在 AI 产业链上，AI 就绪度评级「无」，本维度对论点无加分（N/A）。",
            py_str_py(&name),
            py_str_py(&code)
        )
    } else {
        let lev_str = if leverage_points.is_empty() {
            "—".to_string()
        } else {
            leverage_points
                .iter()
                .map(|p| py_str_py(uzi_core::py::get(p, "category")))
                .collect::<Vec<_>>()
                .join("、")
        };
        format!(
            "{}（{}）AI 暴露评级「{}」（卡位强度 {:.0}/100），通过 {}/3 道 gate → {}；主要 AI 杠杆点：{}。",
            py_str_py(&name),
            py_str_py(&code),
            rating,
            choke,
            passed,
            verdict,
            lev_str
        )
    };

    json!({
        "method": "AI Readiness (single-stock) · 改编自 PE ai-readiness + 复用 ai_chokepoint_score",
        "company": {"name": name, "code": code},
        "generated_at": clock::date_str(&clock::now()),
        "rating": rating,
        "rating_note": rating_note,
        "ai_chokepoint_score": crate::num_value(choke),
        "ai_smallcap": ai_smallcap,
        "gates": gates,
        "gates_passed": passed,
        "gates_total": 3,
        "verdict": verdict,
        "verdict_note": verdict_note,
        "gaps": gaps,
        "leverage_points": leverage_points,
        "conclusion": conclusion,
        // Portfolio-level dimensions · N/A for a single stock
        "cross_portfolio_ranking": "N/A · 单票评估，无跨公司排序（原版 Step 3）",
        "replays": "N/A · 单票评估，无跨公司可复用 playbook（原版 Step 4）",
        "aggregate_ebitda": "N/A · 单票评估，无组合级 EBITDA 汇总（原版 Step 5）",
        "methodology_log": [
            format!("Step 1 · 复用 Serenity ai_chokepoint_score = {:.0}/100（卡位强度锚）", choke),
            format!(
                "Step 2 · Gate① AI 链命中={} · Gate② 真实需求={} · Gate③ 不可替代={}",
                if ai_chain_hit { "True" } else { "False" },
                if has_real_demand { "True" } else { "False" },
                if ai_irreplaceable { "True" } else { "False" }
            ),
            format!("Step 3 · 通过 {}/3 → 评级「{}」· {}", passed, rating, verdict),
            format!("Step 4 · 推断 {} 个 AI 杠杆点", leverage_points.len()),
            "Step 5 · 组合排序/replays/EBITDA 汇总 → 单票 N/A",
        ],
    })
}
