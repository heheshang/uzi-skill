//! Port of `lib/report/special_cards.py` — friendly layer, fund-manager panel,
//! panel insights, school scores, and debate rounds.

use crate::pyfmt::{disp, group_i, num};
use crate::security::{escape_payload, safe_asset_id, safe_url_default};
use crate::svg::*;
use serde_json::Value;
use std::collections::BTreeMap;

fn safe(v: &Value, default: &str) -> String {
    if v.is_null() {
        return default.to_string();
    }
    if let Value::String(s) = v {
        if s.is_empty() || s == "—" {
            return default.to_string();
        }
    }
    disp(v)
}

fn alist(v: &Value, k: &str) -> Vec<Value> {
    match v.get(k) {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

/// Three Tier-4 cards: 一万块场景模拟 / 最像的票 / 离场触发条件.
pub fn render_friendly_layer(syn: &Value, raw: &Value) -> String {
    let syn = escape_payload(syn);
    let _raw = escape_payload(raw);
    let friendly = syn.get("friendly").cloned().unwrap_or(Value::Null);

    let scenarios = friendly.get("scenarios").cloned().unwrap_or(Value::Null);
    let entry_price = scenarios.get("entry_price").cloned().unwrap_or(Value::Number(0.into()));
    let cases = alist(&scenarios, "cases");
    let mut scenario_rows = String::new();
    if !cases.is_empty() {
        for c in &cases {
            let name = disp(c.get("name").unwrap_or(&Value::String(String::new())));
            let prob = disp(c.get("probability").unwrap_or(&Value::String(String::new())));
            let ret_v = c.get("return").cloned().unwrap_or(Value::Number(0.into()));
            let ret = num(&ret_v);
            let val_1w = (10000.0 * (1.0 + ret / 100.0)) as i64;
            let cls = if ret > 0.0 {
                "up"
            } else if ret < 0.0 {
                "down"
            } else {
                "flat"
            };
            let sign = if ret > 0.0 { "+" } else { "" };
            scenario_rows.push_str(&format!(
                r##"<div class="scenario-row"><span class="label">{name} · {prob}</span><span class="val {cls}">{sign}{ret_s}% → ¥{val}</span></div>"##,
                ret_s = disp(&ret_v),
                val = group_i(val_1w)
            ));
        }
    }
    let entry_html = if uzi_core::py::truthy(&entry_price) {
        format!(
            r##"<div style="font-size:11px;color:#475569;margin-bottom:8px">按入场价 <strong>¥{}</strong> 计算：</div>"##,
            disp(&entry_price)
        )
    } else {
        String::new()
    };
    let scenario_card = format!(
        r##"<div class="friendly-card scenario">
  <div class="fc-icon">💰</div>
  <div class="fc-title">如果现在买 1 万块</div>
  <div class="fc-body">
    {entry_html}
    {body}
  </div>
</div>"##,
        body = if scenario_rows.is_empty() {
            r##"<div style="color:#94a3b8;font-size:11px">暂无情景模拟</div>"##.to_string()
        } else {
            scenario_rows
        }
    );

    let similar = alist(&friendly, "similar_stocks");
    let mut similar_pills = String::new();
    for s in similar.iter().take(4) {
        let name = disp(s.get("name").unwrap_or(&Value::String(String::new())));
        let code = disp(s.get("code").unwrap_or(&Value::String(String::new())));
        let similarity = disp(s.get("similarity").unwrap_or(&Value::String(String::new())));
        let reason = disp(s.get("reason").unwrap_or(&Value::String(String::new())));
        let default_url = if !code.is_empty() {
            format!("https://xueqiu.com/S/{code}")
        } else {
            "#".to_string()
        };
        let url = safe_url_default(
            s.get("url")
                .unwrap_or(&Value::String(default_url)),
        );
        similar_pills.push_str(&format!(
            r##"<a href="{url}" target="_blank" rel="noopener" class="similar-stock-pill">
  <div style="display:flex;justify-content:space-between;align-items:baseline">
    <span class="ss-name">{name}</span>
    <span class="ss-meta">相似度 {similarity}</span>
  </div>
  <div class="ss-reason">{reason}</div>
</a>"##
        ));
    }
    let similar_card = format!(
        r##"<div class="friendly-card similar">
  <div class="fc-icon">🔗</div>
  <div class="fc-title">跟它最像的另外几只票</div>
  <div class="fc-body">
    {body}
  </div>
</div>"##,
        body = if similar_pills.is_empty() {
            r##"<div style="color:#94a3b8;font-size:11px">暂无可比股</div>"##.to_string()
        } else {
            similar_pills
        }
    );

    let triggers = alist(&friendly, "exit_triggers");
    let trigger_items: String = triggers
        .iter()
        .map(|t| format!(r##"<div class="exit-trigger-item">{}</div>"##, disp(t)))
        .collect();
    let exit_card = format!(
        r##"<div class="friendly-card exit">
  <div class="fc-icon">🚪</div>
  <div class="fc-title">出现这些信号就离场</div>
  <div class="fc-body">
    {body}
  </div>
</div>"##,
        body = if trigger_items.is_empty() {
            r##"<div style="color:#94a3b8;font-size:11px">暂无触发条件</div>"##.to_string()
        } else {
            trigger_items
        }
    );

    format!("{scenario_card}{similar_card}{exit_card}")
}

// ─── 基金经理抄作业面板 ───

/// Render fund manager performance cards.
pub fn render_fund_managers(managers: &Value) -> String {
    let managers = escape_payload(managers);
    let list = match managers.as_array() {
        Some(a) if !a.is_empty() => a.clone(),
        _ => {
            return r##"<div style="padding:24px;text-align:center;color:#94a3b8;font-size:12px">暂无公募基金持仓数据</div>"##.to_string()
        }
    };

    let is_full = |m: &Value| -> bool {
        m.get("_row_type").and_then(|v| v.as_str()) == Some("full")
            || m.get("return_5y").map(|v| !v.is_null()).unwrap_or(false)
    };
    let mut managers_sorted = list.clone();
    managers_sorted.sort_by(|a, b| {
        let fa = is_full(a);
        let fb = is_full(b);
        let ra = if fa {
            num(a.get("return_5y").unwrap_or(&Value::Number(0.into())))
        } else {
            0.0
        };
        let rb = if fb {
            num(b.get("return_5y").unwrap_or(&Value::Number(0.into())))
        } else {
            0.0
        };
        let pa = num(a.get("position_pct").unwrap_or(&Value::Number(0.into())));
        let pb = num(b.get("position_pct").unwrap_or(&Value::Number(0.into())));
        (if fa { 0 } else { 1 })
            .cmp(&(if fb { 0 } else { 1 }))
            .then_with(|| {
                (-ra)
                    .partial_cmp(&(-rb))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| (-pa).partial_cmp(&(-pb)).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut cards: Vec<String> = Vec::new();
    for m in &managers_sorted {
        let lite = m.get("_row_type").and_then(|v| v.as_str()) == Some("lite")
            || m.get("return_5y").map(|v| v.is_null()).unwrap_or(true);
        if lite {
            continue;
        }
        let name = disp(m.get("name").unwrap_or(&Value::String("—".to_string())));
        let fund_name = disp(m.get("fund_name").unwrap_or(&Value::String("—".to_string())));
        let avatar = safe_asset_id(m.get("avatar").unwrap_or(&Value::Null), "");
        let position = disp(m.get("position_pct").unwrap_or(&Value::Number(0.into())));
        let rank = disp(m.get("rank_in_fund").unwrap_or(&Value::Number(0.into())));
        let quarters = disp(m.get("holding_quarters").unwrap_or(&Value::Number(0.into())));
        let trend = m
            .get("position_trend")
            .and_then(|v| v.as_str())
            .unwrap_or("持平");
        let trend_color = if trend == "加仓" {
            COLOR_BULL
        } else if trend == "减仓" {
            COLOR_BEAR
        } else {
            COLOR_MUTED
        };
        let trend_icon = if trend == "加仓" {
            "📈"
        } else if trend == "减仓" {
            "📉"
        } else {
            "➡️"
        };

        let ret_5y = num(m.get("return_5y").unwrap_or(&Value::Number(0.into())));
        let ann_5y = num(m.get("annualized_5y").unwrap_or(&Value::Number(0.into())));
        let max_dd = num(m.get("max_drawdown").unwrap_or(&Value::Number(0.into())));
        let sharpe = num(m.get("sharpe").unwrap_or(&Value::Number(0.into())));
        let peer_rank = num(m.get("peer_rank_pct").unwrap_or(&Value::Number(50.into())));

        let nav = alist(m, "nav_history");
        let nav_spark = if nav.is_empty() {
            String::new()
        } else {
            let color = if num(&nav[nav.len() - 1]) > num(&nav[0]) {
                COLOR_BULL
            } else {
                COLOR_BEAR
            };
            svg_sparkline(&nav, 280, 50, color, true)
        };

        let ret_color = if ret_5y > 0.0 { COLOR_BULL } else { COLOR_BEAR };
        let dd_color = if max_dd > -20.0 {
            COLOR_BULL
        } else if max_dd > -40.0 {
            COLOR_GOLD
        } else {
            COLOR_BEAR
        };
        let sharpe_color = if sharpe > 1.0 {
            COLOR_BULL
        } else if sharpe > 0.5 {
            COLOR_GOLD
        } else {
            COLOR_BEAR
        };
        let rank_color = if peer_rank < 20.0 {
            COLOR_BULL
        } else if peer_rank < 50.0 {
            COLOR_GOLD
        } else {
            COLOR_BEAR
        };

        let avatar_html = if !avatar.is_empty() {
            format!(
                r##"<img src="avatars/{avatar}.svg" style="width:54px;height:54px;image-rendering:pixelated;border:2px solid #d97706;border-radius:8px;background:#fff;flex-shrink:0">"##
            )
        } else {
            let initial = name.chars().next().map(|c| c.to_string()).unwrap_or_else(|| "?".to_string());
            format!(
                r##"<div style="width:54px;height:54px;background:#fef3c7;border:2px solid #d97706;border-radius:8px;display:flex;align-items:center;justify-content:center;font-family:Fira Sans;font-size:20px;font-weight:900;color:#d97706;flex-shrink:0">{initial}</div>"##
            )
        };

        let stars_n = (((100.0 - peer_rank) / 20.0) as i64 + 1).clamp(1, 5) as usize;
        let stars = "⭐".repeat(stars_n);

        let default_url = format!(
            "https://fund.eastmoney.com/{}.html",
            disp(m.get("fund_code").unwrap_or(&Value::String(String::new())))
        );
        let fund_url = safe_url_default(m.get("fund_url").unwrap_or(&Value::String(default_url)));

        cards.push(format!(
            r##"<div class="fund-card">
  <div class="fund-header">
    {avatar_html}
    <div style="flex:1;min-width:0">
      <div class="fund-manager-name">{name} <span class="fund-stars">{stars}</span></div>
      <div class="fund-name">{fund_name}</div>
      <div class="fund-meta">持本股 {quarters} 季 · 位列第 {rank} 大 · 占基金 {position}% · <span style="color:{trend_color};font-weight:700">{trend_icon} {trend}</span></div>
    </div>
  </div>

  <div class="fund-metrics-grid">
    <div class="fund-metric">
      <div class="fm-label">5 年累计</div>
      <div class="fm-value" style="color:{ret_color}">{ret_sign}{ret_5y:.1}%</div>
    </div>
    <div class="fund-metric">
      <div class="fm-label">年化</div>
      <div class="fm-value">{ann_sign}{ann_5y:.1}%</div>
    </div>
    <div class="fund-metric">
      <div class="fm-label">最大回撤</div>
      <div class="fm-value" style="color:{dd_color}">{max_dd:.1}%</div>
    </div>
    <div class="fund-metric">
      <div class="fm-label">夏普比率</div>
      <div class="fm-value" style="color:{sharpe_color}">{sharpe:.2}</div>
    </div>
  </div>

  <div class="fund-nav-block">
    <div style="display:flex;justify-content:space-between;font-family:Fira Code;font-size:10px;color:#64748b;margin-bottom:4px">
      <span>5 年净值走势</span>
      <span>同类排名 <strong style="color:{rank_color}">前 {peer_rank}%</strong></span>
    </div>
    {nav_spark}
  </div>

  <div style="display:flex;gap:8px;margin-top:10px">
    <a href="{fund_url}" target="_blank" rel="noopener" class="fund-link">查看基金 →</a>
  </div>
</div>"##,
            ret_sign = if ret_5y > 0.0 { "+" } else { "" },
            ann_sign = if ann_5y > 0.0 { "+" } else { "" },
            peer_rank = disp(m.get("peer_rank_pct").unwrap_or(&Value::Number(50.into()))),
        ));
    }

    let full_count = list
        .iter()
        .filter(|m| {
            m.get("_row_type").and_then(|v| v.as_str()) == Some("full")
                || m.get("return_5y").map(|v| !v.is_null()).unwrap_or(false)
        })
        .count();
    let lite_count = list.len() - full_count;
    let header = if lite_count > 0 {
        format!(
            r##"<div class="fund-mgr-header">✨ <strong>{total} 家公募基金</strong>持有本股 · 头部 <strong>{full_count}</strong> 家有完整 5Y 业绩（按收益排序），其余 <strong>{lite_count}</strong> 家按持仓占比列出（点基金链接看详情）</div>"##,
            total = list.len()
        )
    } else {
        format!(
            r##"<div class="fund-mgr-header">✨ <strong>{n} 位公募基金经理</strong>持有本股 · 按 5 年累计收益排序 · 你可以直接"抄作业"</div>"##,
            n = list.len()
        )
    };

    let initial_show = 6usize.min(cards.len());
    let lite_managers: Vec<Value> = managers_sorted
        .iter()
        .filter(|m| {
            m.get("_row_type").and_then(|v| v.as_str()) == Some("lite")
                || m.get("return_5y").map(|v| v.is_null()).unwrap_or(true)
        })
        .cloned()
        .collect();

    let mut lite_sorted = lite_managers.clone();
    lite_sorted.sort_by(|a, b| {
        let pa = num(a.get("position_pct").unwrap_or(&Value::Number(0.into())));
        let pb = num(b.get("position_pct").unwrap_or(&Value::Number(0.into())));
        (-pa).partial_cmp(&(-pb)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen: Vec<String> = Vec::new();
    let mut deduped: Vec<Value> = Vec::new();
    for m in lite_sorted {
        let code = disp(m.get("fund_code").unwrap_or(&Value::Null));
        if seen.contains(&code) {
            continue;
        }
        seen.push(code);
        deduped.push(m);
    }
    const LITE_CAP: usize = 30;
    let lite_capped: Vec<Value> = deduped.iter().take(LITE_CAP).cloned().collect();
    let lite_overflow = deduped.len().saturating_sub(LITE_CAP);

    if lite_managers.is_empty() {
        return format!(
            r##"{header}<div class="fund-mgr-grid">{cards}</div>"##,
            cards = cards.concat()
        );
    }

    let visible = cards[..initial_show].concat();
    let compact_rows: String = lite_capped
        .iter()
        .enumerate()
        .map(|(i, m)| render_fund_compact_row(m, i + 1 + cards.len()))
        .collect();
    let hidden_count = if lite_overflow > 0 {
        format!(
            "{}（另有 {} 家 · 点基金链接自行查）",
            lite_capped.len(),
            lite_overflow
        )
    } else {
        lite_capped.len().to_string()
    };
    // Upstream uses `abs(hash(str(len(cards))))`, which is randomized per process;
    // the port picks a stable identifier instead (see golden README).
    let uid = format!("fm_{}", cards.len());

    format!(
        r##"{header}
    <div class="fund-mgr-grid">{visible}</div>
    <div id="{uid}" class="fund-compact-list" style="display:none">
      <div class="fund-compact-head">
        <span class="fc-h-rank">#</span>
        <span class="fc-h-avatar"></span>
        <span class="fc-h-name">基金经理 / 基金</span>
        <span class="fc-h-metric">5Y 累计</span>
        <span class="fc-h-metric">同类排名</span>
        <span class="fc-h-link"></span>
      </div>
      {compact_rows}
    </div>
    <div style="text-align:center;margin:16px 0">
      <button onclick="var el=document.getElementById('{uid}');var btn=this;if(el.style.display==='none'){{el.style.display='block';btn.textContent='收起 ▲'}}else{{el.style.display='none';btn.textContent='展开剩余 {hidden_count} 位（按 5Y 收益排名）▼'}}"
        style="background:#f59e0b;color:#fff;border:none;padding:10px 28px;border-radius:8px;font-size:14px;font-weight:700;cursor:pointer;transition:all 0.2s">
        展开剩余 {hidden_count} 位（按 5Y 收益排名）▼
      </button>
    </div>"##
    )
}

/// One-line strip for fund managers ranked 7+.
pub fn render_fund_compact_row(m: &Value, rank: usize) -> String {
    let m = escape_payload(m);
    let is_lite = m.get("_row_type").and_then(|v| v.as_str()) == Some("lite")
        || m.get("return_5y").map(|v| v.is_null()).unwrap_or(true);
    let name = disp(m.get("name").unwrap_or(&Value::String("—".to_string())));
    let fund_name = disp(m.get("fund_name").unwrap_or(&Value::String("—".to_string())));
    let fund_code = disp(m.get("fund_code").unwrap_or(&Value::String(String::new())));
    let avatar = safe_asset_id(m.get("avatar").unwrap_or(&Value::Null), "");
    let position_pct = num(m.get("position_pct").unwrap_or(&Value::Number(0.into())));

    let badge_style = if rank <= 3 {
        "background:linear-gradient(135deg,#f59e0b,#d97706);color:#fff"
    } else if rank <= 10 {
        "background:#e2e8f0;color:#475569"
    } else {
        "background:#f1f5f9;color:#64748b"
    };

    let avatar_html = if !avatar.is_empty() {
        format!(r##"<img src="avatars/{avatar}.svg" class="fc-avatar" alt="">"##)
    } else {
        let initial = if !name.is_empty() && name != "—" {
            name.chars().next().map(|c| c.to_string()).unwrap_or_default()
        } else {
            "?".to_string()
        };
        format!(r##"<div class="fc-avatar fc-avatar-ph">{initial}</div>"##)
    };

    let default_url = format!("https://fund.eastmoney.com/{fund_code}.html");
    let fund_url = safe_url_default(m.get("fund_url").unwrap_or(&Value::String(default_url)));

    let (metric_html, name_display, fund_display) = if is_lite {
        (
            format!(
                r##"<span class="fc-return" style="color:#94a3b8;font-style:italic">持仓 {position_pct:.2}%</span><span class="fc-rank-pct" style="color:#94a3b8;font-size:10px">点→查业绩</span>"##
            ),
            fund_name.clone(),
            format!("代码 {fund_code}"),
        )
    } else {
        let ret_5y = num(m.get("return_5y").unwrap_or(&Value::Number(0.into())));
        let peer_rank = num(m.get("peer_rank_pct").unwrap_or(&Value::Number(50.into())));
        let ret_color = if ret_5y > 0.0 { COLOR_BULL } else { COLOR_BEAR };
        let rank_color = if peer_rank < 20.0 {
            COLOR_BULL
        } else if peer_rank < 50.0 {
            COLOR_GOLD
        } else {
            COLOR_BEAR
        };
        let sign = if ret_5y > 0.0 { "+" } else { "" };
        (
            format!(
                r##"<span class="fc-return" style="color:{ret_color}">{sign}{ret_5y:.1}%</span><span class="fc-rank-pct" style="color:{rank_color}">前 {pr}%</span>"##,
                pr = disp(m.get("peer_rank_pct").unwrap_or(&Value::Number(50.into())))
            ),
            name.clone(),
            fund_name.clone(),
        )
    };

    format!(
        r##"<div class="fund-compact-row">
  <span class="fc-rank" style="{badge_style}">{rank}</span>
  {avatar_html}
  <div class="fc-info">
    <div class="fc-name">{name_display}</div>
    <div class="fc-fund">{fund_display}</div>
  </div>
  {metric_html}
  <a href="{fund_url}" target="_blank" rel="noopener" class="fc-link" title="查看基金详情">→</a>
</div>"##
    )
}

/// Panel insight summary (agent content or auto-aggregated).
pub fn render_panel_insights(syn: &Value, panel: &Value) -> String {
    let syn = escape_payload(syn);
    let panel = escape_payload(panel);
    let mut insights = match syn.get("panel_insights") {
        Some(v) if uzi_core::py::truthy(v) => disp(v),
        _ => String::new(),
    };

    let tag_src;
    if insights.is_empty() {
        let sig = panel.get("signal_distribution").cloned().unwrap_or(Value::Null);
        let bull_v = sig.get("bullish").cloned().unwrap_or(Value::Number(0.into()));
        let neu_v = sig.get("neutral").cloned().unwrap_or(Value::Number(0.into()));
        let bear_v = sig.get("bearish").cloned().unwrap_or(Value::Number(0.into()));
        let skip_v = sig.get("skip").cloned().unwrap_or(Value::Number(0.into()));
        let bull = crate::pyfmt::num(&bull_v);
        let bear = crate::pyfmt::num(&bear_v);
        let _ = (crate::pyfmt::num(&neu_v), crate::pyfmt::num(&skip_v));
        let cons = crate::pyfmt::num(
            syn.get("panel_consensus")
                .or_else(|| panel.get("panel_consensus"))
                .unwrap_or(&Value::Number(0.into())),
        );
        let investors = panel.get("investors").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        // group → insertion-ordered signal counts
        let mut grp_stance: BTreeMap<String, Vec<(String, i64)>> = BTreeMap::new();
        for inv in &investors {
            if inv.get("mandate").and_then(|v| v.as_str()) == Some("short") {
                continue;
            }
            let g = disp(inv.get("group").unwrap_or(&Value::String("?".to_string())));
            let s = disp(inv.get("signal").unwrap_or(&Value::String("?".to_string())));
            let entry = grp_stance.entry(g).or_default();
            if let Some(e) = entry.iter_mut().find(|(k, _)| *k == s) {
                e.1 += 1;
            } else {
                entry.push((s, 1));
            }
        }
        let group_labels: &[(&str, &str)] = &[
            ("A", "价值派"),
            ("B", "成长派"),
            ("C", "宏观派"),
            ("D", "技术派"),
            ("E", "中国价投"),
            ("F", "A 股游资"),
            ("G", "量化"),
            ("H", "科技领袖派"),
            ("I", "AI 卡位猎手"),
        ];
        let label_of = |g: &str| -> String {
            group_labels
                .iter()
                .find(|(k, _)| *k == g)
                .map(|(_, v)| (*v).to_string())
                .unwrap_or_else(|| g.to_string())
        };
        let count_of = |c: &Vec<(String, i64)>, key: &str| -> i64 {
            c.iter().find(|(k, _)| k == key).map(|(_, v)| *v).unwrap_or(0)
        };
        let mut grp_summary: Vec<String> = Vec::new();
        for (g, c) in &grp_stance {
            let mut dominant = ("—".to_string(), 0i64);
            for (k, n) in c {
                if *n > dominant.1 {
                    dominant = (k.clone(), *n);
                }
            }
            let label = label_of(g);
            let tag = match dominant.0.as_str() {
                "bullish" => "看多",
                "bearish" => "看空",
                "neutral" => "中性",
                "skip" => "跳过",
                other => other,
            };
            grp_summary.push(format!(
                "{label} {}✓ / {}✗（主流 {tag}）",
                count_of(c, "bullish"),
                count_of(c, "bearish")
            ));
        }
        insights = format!(
            "<strong>51 位评委投票聚合</strong>：{bull_s} 看多 · {neu_s} 中性 · {bear_s} 看空 · {skip_s} 不适合该市场。共识度 <strong>{cons:.0}%</strong>（neutral 半权计入）。<br><br><strong>按流派分布</strong>：{summary}。",
            bull_s = disp(&bull_v),
            neu_s = disp(&neu_v),
            bear_s = disp(&bear_v),
            skip_s = disp(&skip_v),
            cons = cons,
            summary = grp_summary.join("；")
        );
        let short = panel.get("short_consensus").cloned().unwrap_or(Value::Null);
        if uzi_core::py::truthy(short.get("total").unwrap_or(&Value::Null)) {
            insights.push_str(&format!(
                "<br><br><strong>做空派独立观察</strong>：{} 个做空候选 · {} 个暂无明确做空逻辑。",
                disp(short.get("short_candidates").unwrap_or(&Value::Number(0.into()))),
                disp(short.get("no_short_thesis").unwrap_or(&Value::Number(0.into())))
            ));
        }
        if bull == 0.0 && bear > 10.0 {
            insights.push_str(" <em>⚠️ 无一人看多，压倒性看空——高信念回避信号。</em>");
        } else if bear == 0.0 && bull > 10.0 {
            insights.push_str(" <em>⚡ 无一人看空，压倒性看多——共识度极高（警惕追高）。</em>");
        } else if (bull - bear).abs() < 5.0 && (bull + bear) > 20.0 {
            insights.push_str(" <em>🌪 多空旗鼓相当——这类分歧票往往波动最大。</em>");
        }
        tag_src = "（自动聚合 · agent 未介入）";
    } else {
        tag_src = "（agent 深度分析）";
    }

    format!(
        r##"<div class="panel-insights" style="margin:20px 0;padding:20px;background:rgba(8,145,178,0.08);border-left:4px solid #0891b2;border-radius:6px;line-height:1.8;font-size:14px"><div style="font-size:11px;color:#0891b2;letter-spacing:2px;margin-bottom:8px">📊 PANEL INSIGHTS · 评委汇总观点 {tag_src}</div><div>{insights}</div></div>"##
    )
}

pub fn render_school_scores(syn: &Value, panel: &Value) -> String {
    let syn = escape_payload(syn);
    let panel = escape_payload(panel);
    let school = match syn.get("school_scores") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => match panel.get("school_scores") {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => Value::Null,
        },
    };
    if !uzi_core::py::truthy(&school) {
        return String::new();
    }

    let verdict_color = |v: &str| -> (&'static str, &'static str) {
        match v {
            "重仓" => ("#065f46", "rgba(16,185,129,0.15)"),
            "买入" => ("#047857", "rgba(16,185,129,0.10)"),
            "关注" => ("#b45309", "rgba(245,158,11,0.10)"),
            "谨慎" => ("#b91c1c", "rgba(239,68,68,0.10)"),
            "回避" => ("#991b1b", "rgba(239,68,68,0.18)"),
            "不适合" => ("#6b7280", "rgba(107,114,128,0.10)"),
            _ => ("#374151", "rgba(107,114,128,0.10)"),
        }
    };
    let sig_icon = |s: &str| -> &'static str {
        match s {
            "bullish" => "📈",
            "bearish" => "📉",
            "neutral" => "⚖️",
            "skip" => "—",
            _ => "",
        }
    };

    let mut items: Vec<String> = Vec::new();
    for g in ["A", "B", "C", "D", "E", "F", "G", "H", "I"] {
        let s = match school.get(g) {
            Some(v) if uzi_core::py::truthy(v) => v.clone(),
            _ => continue,
        };
        let label = match s.get("label") {
            Some(v) if uzi_core::py::truthy(v) => disp(v),
            _ => g.to_string(),
        };
        let cons_v = s.get("consensus").cloned().unwrap_or(Value::Number(0.into()));
        let cons = crate::pyfmt::num(&cons_v);
        let avg = crate::pyfmt::num(s.get("avg_score").unwrap_or(&Value::Number(0.into())));
        let score_mean = s.get("score_mean").map(crate::pyfmt::num).unwrap_or(avg);
        let vote_cons = s
            .get("vote_consensus")
            .map(crate::pyfmt::num)
            .unwrap_or(cons);
        let verdict = s.get("verdict").and_then(|v| v.as_str()).unwrap_or("—");
        let n_members = disp(s.get("n_members").unwrap_or(&Value::Number(0.into())));
        let bull = disp(s.get("bullish").unwrap_or(&Value::Number(0.into())));
        let neu = disp(s.get("neutral").unwrap_or(&Value::Number(0.into())));
        let bear = disp(s.get("bearish").unwrap_or(&Value::Number(0.into())));
        let skip_v = s.get("skip").cloned().unwrap_or(Value::Number(0.into()));
        let skip = crate::pyfmt::num(&skip_v);
        let desc = disp(s.get("desc").unwrap_or(&Value::String(String::new())));
        let dom = s.get("dominant_signal").and_then(|v| v.as_str()).unwrap_or("skip");
        let (fg, bg) = verdict_color(verdict);
        let icon = sig_icon(dom);

        // Python `max(0, min(100, cons))` keeps the original int/float identity.
        let bar_fill = if cons >= 100.0 {
            "100".to_string()
        } else if cons < 0.0 {
            "0".to_string()
        } else {
            disp(&cons_v)
        };
        let tip = format!(
            "score_mean={sm:.1} · vote_weighted={vc:.1} · 极化后 {cons:.1}",
            sm = score_mean,
            vc = vote_cons,
            cons = cons
        );
        let skip_display = if skip != 0.0 {
            format!(r##"· <span style="color:#9ca3af">—{}</span>"##, disp(&skip_v))
        } else {
            String::new()
        };
        items.push(format!(
            r##"<div title="{tip}" style="background:{bg};border-radius:8px;padding:14px 16px;border:1px solid rgba(0,0,0,0.05)">  <div style="display:flex;justify-content:space-between;align-items:baseline">    <div style="font-weight:600;font-size:14px;color:{fg}">      {icon} {label} <span style="font-weight:400;font-size:11px;color:#9ca3af">· {n_members} 人</span>    </div>    <div style="font-size:11px;color:{fg};font-weight:600;letter-spacing:1px">{verdict}</div>  </div>  <div style="margin-top:6px;font-size:11px;color:#6b7280">{desc}</div>  <div style="display:flex;gap:12px;margin-top:10px;align-items:center">    <div style="flex:1">      <div style="height:6px;background:rgba(0,0,0,0.06);border-radius:3px;overflow:hidden">        <div style="height:100%;width:{bar_fill}%;background:linear-gradient(90deg,{fg} 0%,{fg} 100%);opacity:0.75"></div>      </div>      <div style="font-size:10px;color:#9ca3af;margin-top:3px">        流派分 <strong style="color:{fg};font-size:12px">{cons:.1}</strong>        <span style="color:#d1d5db"> · 实分均值 {score_mean:.1} · 投票共识 {vote_cons:.0}%</span>      </div>    </div>    <div style="font-size:11px;color:#374151;white-space:nowrap">      <span style="color:#059669">📈{bull}</span> ·       <span style="color:#6b7280">⚖️{neu}</span> ·       <span style="color:#dc2626">📉{bear}</span>      {skip_display}    </div>  </div></div>"##,
        ));
    }

    if items.is_empty() {
        return String::new();
    }

    format!(
        r##"<div class="school-scores" style="margin:20px 0;padding:20px;background:rgba(139,92,246,0.06);border-left:4px solid #8b5cf6;border-radius:6px">  <div style="font-size:11px;color:#7c3aed;letter-spacing:2px;margin-bottom:4px">🎭 SCHOOL SCORES · 七大流派各自评分</div>  <div style="font-size:12px;color:#6b7280;margin-bottom:14px">混合打分 = 0.65 × 实分均值 + 0.35 × 投票共识 · 再做极化拉伸(k=1.3) · 不同哲学给出不同分数 · 分歧越大意味着结论越不稳 · 鼠标悬停查看分量  </div>  <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(300px,1fr));gap:12px">{items}  </div></div>"##,
        items = items.concat()
    )
}

pub fn render_debate_rounds(debate: &Value) -> String {
    let debate = escape_payload(debate);
    let rounds = alist(&debate, "rounds");
    if rounds.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for r in &rounds {
        let rn = disp(r.get("round").unwrap_or(&Value::Null));
        let bull_say = safe(r.get("bull_say").unwrap_or(&Value::Null), "—");
        let bear_say = safe(r.get("bear_say").unwrap_or(&Value::Null), "—");
        out.push(format!(
            r##"<div class="round">
  <div class="round-label">ROUND {rn}</div>
  <div class="round-grid">
    <div class="round-bull">{bull_say}</div>
    <div class="round-vs">VS</div>
    <div class="round-bear">{bear_say}</div>
  </div>
</div>"##
        ));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Mirrors upstream tests/test_html_escape_boundary.py::
    //   test_special_renderer_rejects_javascript_urls
    // Python: rendered = render_friendly_layer({"friendly": {"similar_stocks": [{"name": "测试", "code": "TEST", "url": "javascript:alert(1)"}]}}, {})
    //   assert 'href="#"' in rendered; "javascript:" not in rendered
    #[test]
    fn special_renderer_rejects_javascript_urls() {
        let rendered = render_friendly_layer(
            &json!({"friendly": {"similar_stocks": [{
                "name": "测试",
                "code": "TEST",
                "url": "javascript:alert(1)",
            }]}}),
            &json!({}),
        );
        assert!(rendered.contains("href=\"#\""));
        assert!(!rendered.contains("javascript:"));
    }
}
