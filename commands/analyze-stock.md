---
description: 完整深度分析一只股票（22 维数据 + 66 位大佬量化评委 + 22 种机构分析方法 + 杀猪盘检测 + Bloomberg 风格 HTML 报告）
argument-hint: "[股票名称或代码，例如 华工科技 / 002273 / AAPL / 00700.HK]"
---

# 深度分析任务

用户输入: $ARGUMENTS

## 执行流程（两段式 · 你必须在中间介入）

本项目是单一二进制 `uzi`：上游的两段式 stage1 / stage2 对应 `--stage1` / `--stage2`，
22 维采集（上游那批 `fetch_*` 采集脚本）已内建，**由 `--stage1` 自动采集**，不需要手工逐脚本调用。

### 第一段 · 数据采集 + 骨架分（`--stage1` 完成）

```bash
uzi $ARGUMENTS --stage1
```

一把跑完 Task 1 → 1.5 → 2 → 3：

1. **22 维数据采集**（上游 `fetch_*` 脚本 → `uzi_data` 的 HTTP 端点；失败场按
   EastMoney / Tencent / Sina / Yahoo 降级链，再不行走 CDP 浏览器兜底）。
2. **机构建模**（dim 20 估值建模 / dim 21 研究工作流 / dim 22 深度决策）。
3. **规则引擎骨架分 + 66 位评委骨架分**。

产物落在 `.cache/{ticker}/`：

| 文件 | 内容 |
|---|---|
| `raw_data.json` | 22 维原始数据（`dimensions.<dim>.data.<key>`） |
| `dimensions.json` | 22 维评分（`score` / `weight` / `reasons_pass` / `reasons_fail`） |
| `panel.json` | 66 位评委骨架分（`investors` / `vote_distribution` / `panel_consensus`） |
| `_data_gaps.json` | 采集缺口（存在时 agent 必须接管） |
| `_agent_review_context.json` | `analysis_input_hash`（写回时要用） |

想忽略缓存全量重抓：`uzi $ARGUMENTS --no-resume --stage1`。

### 第二段 · 你来分析（核心！不能跳过！）

Stage 1 跑完后，**你必须做以下事情**：

**0. 浏览器兜底前置（必走）**

本项目用系统已装的 Chromium 系浏览器经 **CDP** 驱动，零额外安装。先确认可用：

```bash
uzi --browser-check
```

再读 `.cache/{ticker}/_review_issues.json`（存在时），挑出 `category == "data"` 且
`severity` 为 `critical` / `warning` 的低质量维度；若不为空，用
`uzi $ARGUMENTS --no-resume --stage1` 全量重抓（浏览器兜底已在采集链内自动生效）。
采集始终拿不到的字段，走下面的 `data_gap_acknowledged` 显式标注。

**1. 读取评委骨架分**

```bash
jq '.vote_distribution' .cache/{ticker}/panel.json
jq '[.investors[] | select(.signal=="bullish")] | sort_by(-.score) | .[0:5] | .[] | {name, score, comment}' .cache/{ticker}/panel.json
jq '[.investors[] | select(.signal=="bearish")] | sort_by(.score) | .[0:5] | .[] | {name, score, comment}' .cache/{ticker}/panel.json
```

看 66 人各自打了多少分。特别关注：
- Top 5 看多和 Top 5 看空分别是谁？他们的 `comment` 有没有说服力？
- 有多少人 `signal == "skip"`？（F 组游资不在射程时标 `不适合`）
- 有没有明显不合理的分数？

**2. 逐组分析（spawn 4 个并行 sub-agent）**

对每组投资者，spawn 一个 Agent（分组与人数按 9 大流派，详见
`skills/investor-panel/references/group-{a..i}.md`）：

**Agent 1 · A 经典价值（6）+ B 成长（9）= 15 人**
```
你要扮演 A 组与 B 组评委（巴菲特/格雷厄姆/费雪/芒格/邓普顿/卡拉曼/林奇/欧奈尔/蒂尔/木头姐 等），
逐一对 {stock_name} ({ticker}) 给出判断。

公司数据：{从 raw_data.json 摘取关键数据}
规则引擎参考分：{从 panel.json 摘取这两组的 score/comment}
真实持仓：{巴菲特持有苹果/BYD，段永平持有苹果/茅台/腾讯 等}

对每人输出: investor_id, signal, score(0-100), headline(引用数字), reasoning(2-3句)
你可以覆盖规则引擎的分数——你是在模拟这个人的判断，不是跑公式。
```

**Agent 2 · C 宏观对冲（7）+ D 技术趋势（4）= 11 人**

**Agent 3 · E 中国价投（7）+ G 量化（4）+ H 科技领袖（4）+ I Serenity（1）= 16 人**

**Agent 4 · F A股游资（24 人）** — 非 A 股或不在射程的按 `group-f-china-youzi.md`
射程规则全部标 `不适合`

**3. 合并 agent 结果**

把 4 个 agent 返回的 `{signal, score, headline, reasoning}` 覆盖到
`.cache/{ticker}/panel.json` 的对应投资者上（合计 66 人）。

**4. 写 agent_analysis.json（闭环关键！）**

对关键维度（财报/估值/护城河/行业）写 1-2 句定性评语（≥20 字，引用具体数字）。
如果需要，用 web search / 浏览器补充信息。

**⚠️ 必读：agent_analysis.json 完整 schema（缺字段 stage2 会报 schema warning/error）**

| 字段 | 要求 | 触发校验 |
|---|---|---|
| `agent_reviewed` | 必须 `true` | 🔴 缺失/非 true → 整体拒绝复用 |
| `analysis_input_hash` | 取自 `.cache/{ticker}/_agent_review_context.json`，deep 档必填 | 🔴 deep 缺失 → HARD-GATE 拒绝；不符 → 视为过期 |
| `dim_commentary` | 覆盖全部 22 维，**每条 ≥20 字**（引用具体数字，禁止空泛） | ⚠️ <20 字 → warning |
| `panel_insights` | **≥30 字**，评委投票分布 + 多空分歧分析 | ⚠️ <30 字 → warning |
| `great_divide_override` | punchline(≥10 字) + bull_say_rounds(≥3 条) + bear_say_rounds(≥3 条) | 🔴 缺字段 → error |
| `narrative_override.core_conclusion` | **≥20 字**综合定论 | ⚠️ <20 字 → warning |
| `narrative_override.risks` | **≥3 条**风险 | ⚠️ <3 条 → warning |
| `narrative_override.buy_zones` | **必须含 value/growth/technical/youzi 四个 key**，每个 key 内含 `price`(数值, youzi 可为 0) + `rationale`(≥5 字解释) | 🔴 缺 key → error / ⚠️ 缺子字段 → warning |
| `qualitative_deep_dive` | 覆盖 3_macro/7_industry/8_materials/9_futures/13_policy/15_events 共 6 维。每维含：`evidence` 数组（≥2 条）、`associations` 跨域因果链（6 维合计 ≥3 条）、`conclusion`（1-2 句） | 🔴 evidence 非 list → error |
| `data_gap_acknowledged` | dict 格式 `{"dim_key": "已尝试 X 但失败的原因"}`，标记数据采集失败但 agent 已知晓的维度 | 🔴 类型非 dict → error |

把上述字段写成一个 JSON 对象落到 `.cache/{ticker}/agent_analysis.json`
（结构如下，`agent_reviewed` 必须为 `true`）：

```json
{
  "agent_reviewed": true,
  "analysis_input_hash": "<取自 _agent_review_context.json>",
  "dim_commentary": {
    "0_basic": "公司全称+成立/上市时间+市值+行业地位，≥20字",
    "1_financials": "ROE/营收增速/净利率/毛利率/FCF等核心数据+质量判断，≥20字",
    "...": "覆盖全部 22 维，每条 ≥20 字，引用具体数字"
  },
  "panel_insights": "评委投票分布(看多X/中性X/看空X)+多空分歧核心逻辑，≥30字",
  "great_divide_override": {
    "punchline": "多空对决一句话金句，≥10字",
    "bull_say_rounds": ["R1: 看多论点+引用数字", "R2: ...", "R3: ..."],
    "bear_say_rounds": ["R1: 看空论点+引用数字", "R2: ...", "R3: ..."]
  },
  "narrative_override": {
    "core_conclusion": "综合定论+评分+建仓建议，≥20字",
    "risks": ["风险1", "风险2", "风险3"],
    "buy_zones": {
      "value":     {"price": 140, "rationale": "DCF安全边际>60%，等待极端低估"},
      "growth":    {"price": 160, "rationale": "PEG<0.1极度低估，当前即可建仓"},
      "technical": {"price": 180, "rationale": "等待Stage 2突破确认后右侧入场"},
      "youzi":     {"price": 0, "rationale": "非A股不适用游资打板策略"}
    }
  },
  "qualitative_deep_dive": {
    "3_macro": {
      "evidence": [{"source": "...", "url": "...", "finding": "...", "retrieved_at": "2026-04-27"}],
      "associations": [{"link_to": "7_industry", "chain_id": "macro->industry", "causal_chain": "...", "estimated_impact": "medium"}],
      "conclusion": "宏观结论1-2句"
    },
    "7_industry": {},
    "8_materials": {},
    "9_futures": {},
    "13_policy": {},
    "15_events": {}
  },
  "data_gap_acknowledged": {
    "10_valuation.pe_quantile": "历史分位查询对该市场不支持，已用同行分位替代"
  }
}
```

> 详细说明见 `skills/deep-analysis/SKILL.md` 的「agent_analysis.json 字段与校验」表，
> 以及 `skills/deep-analysis/references/task2.5-qualitative-deep-dive.md` 第 5 节。

### 第三段 · 生成报告（`--stage2` 完成）

```bash
uzi $ARGUMENTS --stage2
```

stage2 会自动读取 `panel.json` + `agent_analysis.json`，合并生成最终报告。
`agent_analysis.json` 中的字段优先级高于脚本 stub。
deep 档若没有当前快照可用的 `agent_analysis.json`，会触发 HARD-GATE 直接拒绝出报告。

### 第四段 · 向用户汇报

1. 综合评分 + 定调
2. 66 评委投票分布
3. DCF 内在价值 vs 当前价
4. Top 3 看多理由 + Top 3 看空理由
5. Great Divide 金句
6. 杀猪盘等级（`18_trap`）
7. 报告文件路径

## 快速模式（跳过 agent 介入）

如果用户说"快速分析"或"不用那么详细"：

```bash
uzi $ARGUMENTS --depth lite --no-browser
```

这会一把跑完 stage1 + stage2，不做 agent 分析（评委为规则引擎输出）。

## 禁止

- 不跑脚本就编造数据
- 跳过 agent 分析直接出报告（除非用户明确要快速模式）
- 用"基本面良好"等模板话术
