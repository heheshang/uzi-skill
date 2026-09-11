//! Port of `lib/report/panel_cards.py` — investor panel rendering cards
//! (judge seats, chat bubbles, vote bars, top-3 bulls/bears, risk list).

use crate::pyfmt::disp;
use crate::security::{escape_payload, safe_asset_id_default};
use serde_json::Value;

/// Local `_safe` helper (avoids the assemble_report import cycle).
pub(crate) fn safe(v: &Value, default: &str) -> String {
    if v.is_null() {
        return default.to_string();
    }
    if let Value::String(s) = v {
        if s.is_empty() || s == "—" || s == "nan" {
            return default.to_string();
        }
    }
    disp(v)
}

/// 流派中文标签 (A-I).
pub const GROUP_LABELS: &[(&str, &str)] = &[
    ("A", "价值"),
    ("B", "成长"),
    ("C", "宏观"),
    ("D", "技术"),
    ("E", "中国"),
    ("F", "游资"),
    ("G", "量化"),
    ("H", "科技"),
    ("I", "卡位"),
];

pub fn group_label(group: &str) -> String {
    GROUP_LABELS
        .iter()
        .find(|(k, _)| *k == group)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| group.to_string())
}

fn get_str<'a>(v: &'a Value, key: &str, default: &'a str) -> &'a str {
    match v.get(key) {
        Some(Value::String(s)) => s,
        _ => default,
    }
}

/// One judge seat on the judging board.
pub fn render_jury_seat(inv: &Value) -> String {
    let inv = escape_payload(inv);
    let sig = get_str(&inv, "signal", "neutral");
    let name_full = disp(inv.get("name").filter(|v| uzi_core::py::truthy(v)).unwrap_or(&Value::String(String::new())));
    let name: String = name_full.chars().take(4).collect();
    let score = disp(inv.get("score").unwrap_or(&Value::Number(0.into())));
    let inv_id = safe_asset_id_default(inv.get("investor_id").unwrap_or(&Value::Null));
    format!(
        r##"<div class="seat {sig}" data-group="{group}" data-target="msg-{inv_id}" title="{title} · {verdict} · 点击查看完整结论">
  <img src="avatars/{inv_id}.svg" class="seat-avatar" alt="">
  <div class="seat-name">{name}</div>
  <div class="seat-score">{score}</div>
</div>"##,
        group = get_str(&inv, "group", ""),
        title = name_full,
        verdict = get_str(&inv, "verdict", ""),
    )
}

/// `<li>` list helper; dict items render `msg`/`name`.
pub fn li(items: &Value) -> String {
    let arr = match items.as_array() {
        Some(a) if !a.is_empty() => a,
        _ => return String::new(),
    };
    arr.iter()
        .map(|x| match x {
            Value::Object(_) => {
                let msg = x.get("msg").unwrap_or(&Value::Null);
                let name = x.get("name").unwrap_or(&Value::Null);
                let text = if uzi_core::py::truthy(msg) {
                    disp(msg)
                } else if uzi_core::py::truthy(name) {
                    disp(name)
                } else {
                    String::new()
                };
                format!("<li>{text}</li>")
            }
            _ => format!("<li>{}</li>", disp(x)),
        })
        .collect()
}

/// One chat bubble + expandable full conclusion.
pub fn render_chat_message(inv: &Value) -> String {
    let inv = escape_payload(inv);
    let sig = get_str(&inv, "signal", "neutral");
    let group = get_str(&inv, "group", "").to_string();
    let group_label = group_label(&group);
    let score = disp(inv.get("score").unwrap_or(&Value::Number(0.into())));
    let confidence = disp(inv.get("confidence").unwrap_or(&Value::Number(0.into())));
    let reasoning = safe(
        match inv.get("reasoning") {
            Some(r) if uzi_core::py::truthy(r) => r,
            _ => inv.get("comment").unwrap_or(&Value::Null),
        },
        "—",
    );
    let comment = safe(inv.get("comment").unwrap_or(&Value::Null), "");
    let verdict = safe(inv.get("verdict").unwrap_or(&Value::Null), "—");
    let pass_items = match inv.get("pass") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Array(vec![]),
    };
    let fail_items = match inv.get("fail") {
        Some(v) if uzi_core::py::truthy(v) => v.clone(),
        _ => Value::Array(vec![]),
    };
    let ideal_price = inv.get("ideal_price").cloned().unwrap_or(Value::Null);
    let period = safe(inv.get("period").unwrap_or(&Value::Null), "—");
    let inv_id = safe_asset_id_default(inv.get("investor_id").unwrap_or(&Value::Null));

    let mut bubble_main = format!(r#"<div class="msg-reasoning">{reasoning}</div>"#);
    if !comment.is_empty() && comment != reasoning {
        bubble_main.push_str(&format!(r#"<div class="msg-comment">💬 "{comment}"</div>"#));
    }

    let pass_html = if uzi_core::py::truthy(&pass_items) {
        format!(
            r##"<div class="conc-block"><div class="conc-label">✅ 命中</div><ul>{}</ul></div>"##,
            li(&pass_items)
        )
    } else {
        String::new()
    };
    let fail_html = if uzi_core::py::truthy(&fail_items) {
        format!(
            r##"<div class="conc-block"><div class="conc-label">❌ 未命中</div><ul>{}</ul></div>"##,
            li(&fail_items)
        )
    } else {
        String::new()
    };
    let price_html = if uzi_core::py::truthy(&ideal_price) {
        format!(
            r##"<div class="conc-row"><span>🎯 理想买入价</span><strong>¥{}</strong></div>"##,
            disp(&ideal_price)
        )
    } else {
        String::new()
    };

    let th = safe(inv.get("time_horizon").unwrap_or(&Value::Null), "");
    let ps = safe(inv.get("position_sizing").unwrap_or(&Value::Null), "");
    let wc = safe(
        inv.get("what_would_change_my_mind").unwrap_or(&Value::Null),
        "",
    );
    let mut profile_rows = Vec::new();
    if !th.is_empty() && th != "—" {
        profile_rows.push(format!(
            r##"<div class="conc-row"><span>⏱ 时间框架</span><em>{th}</em></div>"##
        ));
    }
    if !ps.is_empty() && ps != "—" {
        profile_rows.push(format!(
            r##"<div class="conc-row"><span>💰 仓位风格</span><em>{ps}</em></div>"##
        ));
    }
    if !wc.is_empty() && wc != "—" {
        profile_rows.push(format!(
            r##"<div class="conc-row"><span>🔄 翻盘条件</span><em>{wc}</em></div>"##
        ));
    }
    let profile_html = if !profile_rows.is_empty() {
        format!(
            r##"<div class="conc-block"><div class="conc-label">🧭 我的方法论</div>{}</div>"##,
            profile_rows.concat()
        )
    } else {
        String::new()
    };

    format!(
        r##"<div class="chat-msg {sig}" data-group="{group}" id="msg-{inv_id}">
  <img src="avatars/{inv_id}.svg" class="msg-avatar" alt="">
  <div class="msg-body">
    <div class="msg-meta">
      <span class="msg-name">{name}</span>
      <span class="msg-group-tag">{group} · {group_label}</span>
      <span class="msg-signal-dot"></span>
      <span class="msg-score-badge">{score}分</span>
      <span class="msg-confidence">conf {confidence}</span>
    </div>
    <div class="msg-bubble">
      {bubble_main}
      <div class="msg-verdict">▸ {verdict} · 周期 {period}</div>
      <details class="msg-details">
        <summary>展开完整结论 ▼</summary>
        <div class="conc-content">
          {pass_html}
          {fail_html}
          {price_html}
          {profile_html}
        </div>
      </details>
    </div>
  </div>
</div>"##,
        name = get_str(&inv, "name", ""),
    )
}

/// Vote distribution bars.
pub fn render_vote_bars(vote_dist: &Value) -> String {
    let labels = [
        ("强烈买入", "strongly_buy", "var(--bull-green)"),
        ("买入", "buy", "var(--bull-green)"),
        ("关注", "watch", "var(--neon-gold)"),
        ("观望", "wait", "var(--text-dim)"),
        ("回避", "avoid", "var(--bear-red)"),
    ];
    let total: f64 = vote_dist
        .as_object()
        .map(|o| o.values().map(crate::pyfmt::num).sum())
        .unwrap_or(0.0);
    let total = if total == 0.0 { 1.0 } else { total };
    let mut rows = Vec::new();
    for (cn, key, color) in labels {
        let count = vote_dist
            .get(key)
            .map(crate::pyfmt::num)
            .unwrap_or(0.0);
        let pct = count / total * 100.0;
        rows.push(format!(
            r##"<div class="sc-vote-row"><span style="width: 140px">{cn}</span><div class="bar"><div class="fill" style="width:{pct:.0}%; background:{color}"></div></div><span style="width: 60px; text-align: right">{count} 人</span></div>"##,
            count = disp(vote_dist.get(key).unwrap_or(&Value::Number(0.into())))
        ));
    }
    rows.join("\n")
}

pub fn render_top3_bulls(investors: &Value) -> String {
    render_top3_by_signal(investors, "bullish", "无看多评委 · 51 人整体倾向中性")
}

pub fn render_top3_bears(investors: &Value) -> String {
    render_top3_by_signal(investors, "bearish", "无看空评委 · 51 人整体倾向中性")
}

fn render_top3_by_signal(investors: &Value, target_signal: &str, empty_msg: &str) -> String {
    let investors = escape_payload(investors);
    let empty_msg = escape_payload(&Value::String(empty_msg.to_string()));
    let arr = investors.as_array().cloned().unwrap_or_default();
    let mut hits: Vec<Value> = arr
        .into_iter()
        .filter(|i| {
            get_str(i, "signal", "neutral") == target_signal
                && get_str(i, "mandate", "") != "short"
        })
        .collect();
    hits.sort_by(|a, b| {
        let sa = crate::pyfmt::num(a.get("score").unwrap_or(&Value::Number(0.into())));
        let sb = crate::pyfmt::num(b.get("score").unwrap_or(&Value::Number(0.into())));
        if target_signal == "bullish" {
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        } else {
            sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    hits.truncate(3);
    if hits.is_empty() {
        return format!(
            r##"<div class="sc-best-empty" style="grid-column:1/-1;text-align:center;color:#94a3b8;font-size:12px;padding:16px">{}</div>"##,
            disp(&empty_msg)
        );
    }
    let mut cells: Vec<String> = hits
        .iter()
        .map(|inv| {
            format!(
                r##"<div class="sc-best-cell"><img src="avatars/{}.svg"><div class="name">{}</div><div class="score-num">{}</div></div>"##,
                safe_asset_id_default(inv.get("investor_id").unwrap_or(&Value::Null)),
                disp(inv.get("name").unwrap_or(&Value::Null)),
                disp(inv.get("score").unwrap_or(&Value::Number(0.into())))
            )
        })
        .collect();
    while cells.len() < 3 {
        cells.push(
            r##"<div class="sc-best-cell" style="opacity:0.2"><div style="font-size:12px;color:#94a3b8">—</div></div>"##
                .to_string(),
        );
    }
    cells.join("\n")
}

/// Risk bullet list.
pub fn render_risks(risks: &Value) -> String {
    let risks = escape_payload(risks);
    match risks.as_array() {
        Some(arr) => arr
            .iter()
            .map(|r| format!("<li>{}</li>", disp(r)))
            .collect::<Vec<_>>()
            .join("\n"),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Mirrors upstream tests/test_html_escape_boundary.py::
    //   test_panel_renderer_escapes_roleplay_text_and_asset_id
    // Python: rendered = render_chat_message({"investor_id": "../../evil\" onerror=alert(1)", "name": PAYLOAD, "reasoning": PAYLOAD, "signal": "neutral"})
    //   assert PAYLOAD not in rendered; "../" not in rendered; "&lt;img" in rendered
    const PAYLOAD: &str = "<img src=x onerror=\"alert(1)\">";

    #[test]
    fn panel_renderer_escapes_roleplay_text_and_asset_id() {
        let rendered = render_chat_message(&json!({
            "investor_id": "../../evil\" onerror=alert(1)",
            "name": PAYLOAD,
            "reasoning": PAYLOAD,
            "signal": "neutral",
        }));
        assert!(!rendered.contains(PAYLOAD));
        assert!(!rendered.contains("../"));
        assert!(rendered.contains("&lt;img"));
        // python3 -c "from lib.report.panel_cards import render_chat_message as r; print(r({'investor_id':'../../evil\" onerror=alert(1)','name':'x','signal':'neutral'}))"
        //   -> id="msg-evilquotonerroralert1"
        assert!(rendered.contains("id=\"msg-evilquotonerroralert1\""));
    }
}
