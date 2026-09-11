---
name: deep-analysis
description: 个股深度分析的核心工作流（Rust 实现）。当用户要求"深度分析 / 全面分析 / 帮我看看 / 值不值得买 / DCF / 机构建模 / 首次覆盖 / 投委会备忘录"等涉及个股研究的请求时触发。覆盖 A 股、港股、美股，产出 22 维数据 + 66 位大佬量化评审 + 机构级估值建模（DCF/Comps/LBO/3-Stmt/Merger）+ 研究产物（首次覆盖/财报解读/催化剂日历/投资逻辑追踪/晨报/量化筛选/行业综述）+ 决策方法（IC Memo/DD/Porter/单位经济/VCP/再平衡）+ 杀猪盘检测，最终生成 Bloomberg 风格 HTML 报告 + 社交分享战报。关键词：股票、个股、深度分析、估值、DCF、comps、首次覆盖、IC memo、杀猪盘、龙虎榜。
version: 3.9.4
author: FloatFu-true
license: MIT
metadata:
  tags: [finance, stocks, a-share, hong-kong, us-stocks, dcf, valuation, equity-research, trap-detection]
---

# Stock Deep Analysis · 深度分析工作流

> 你正在扮演一位**首席股票分析师**。你身边有一套完整的量化工具箱，但最终的判断和叙事**必须你来写**。
> 脚本负责算数，你负责推理和下结论。

本仓库是 [UZI-Skill](https://github.com/wbh604/UZI-Skill) `skills/deep-analysis` 的 **Rust 实现**。
上游入口是 `run.py`（Python 脚本）；这里用单一二进制 **`uzi`**。行为由 golden 差分测试对照上游
锁定（见文末「开发与验证」）。

**同仓库的其它三个 skill**（更窄的场景优先用它们）：

| Skill | 何时读 |
|---|---|
| [`../investor-panel/SKILL.md`](../investor-panel/SKILL.md) | 只要评审团投票 / "大佬怎么看" |
| [`../lhb-analyzer/SKILL.md`](../lhb-analyzer/SKILL.md) | 龙虎榜、游资席位、谁在买 |
| [`../trap-detector/SKILL.md`](../trap-detector/SKILL.md) | 杀猪盘、被推荐、安全性排查 |
| [`../../SKILL.md`](../../SKILL.md) | 根索引（不知道选哪个时先读它）|

## 📚 分任务操作手册（`references/`）

本文件是总纲；每个 Task 的**详细操作手册**在 `references/` 下，按需读取：

| 手册 | 内容 |
|---|---|
| [`references/task1-data-collection.md`](references/task1-data-collection.md) | 22 维采集：每个维度的 fetcher、字段、降级链 |
| [`references/data-sources.md`](references/data-sources.md) | 数据源总表：本项目实际使用的 HTTP 端点、优先级规则、TTL |
| [`references/task1.5-institutional-modeling.md`](references/task1.5-institutional-modeling.md) | 机构建模 dim 20/21/22（DCF/Comps/LBO/IC Memo…） |
| [`references/task2-dimension-scoring.md`](references/task2-dimension-scoring.md) | 22 维打分规则与加权公式 |
| [`references/task2.5-qualitative-deep-dive.md`](references/task2.5-qualitative-deep-dive.md) | 6 维定性深挖：问题清单、跨域因果链、输出 schema |
| [`references/task3-agent-evaluation.md`](references/task3-agent-evaluation.md) | 评委 role-play 的分组、sub-agent 编排、自检 |
| [`references/task3-investor-panel.md`](references/task3-investor-panel.md) | `panel.json` 字段与共识公式 |
| [`references/task4-synthesis.md`](references/task4-synthesis.md) | 综合研判与叙事合成 |
| [`references/task5-report-assembly.md`](references/task5-report-assembly.md) | 报告装配：HTML 结构、Dashboard 模板、配色 |
| [`references/fin-methods/`](references/fin-methods/README.md) | 各机构方法单独说明（含 `serenity-bottleneck`） |

## ⚡ 逐方法命令卡（`commands/`）

`commands/*.md` 给出单个方法的操作步骤。所有方法都已接到 CLI，**先建缓存再取结果**：

```bash
uzi <ticker> --stage1              # 前提：跑一次采集+建模+评分
uzi <ticker> --method <NAME>       # stdout 纯 JSON，可直接 jq
```

| 命令卡 | `--method` NAME | 类型 |
|---|---|---|
| [`dcf`](../../commands/dcf.md) · [`comps`](../../commands/comps.md) · [`lbo`](../../commands/lbo.md) | `dcf` · `comps` · `lbo` · `three-statement` | 缓存型（dim 20） |
| [`initiate`](../../commands/initiate.md) · [`earnings`](../../commands/earnings.md) · [`catalysts`](../../commands/catalysts.md) · [`thesis`](../../commands/thesis.md) | `initiate` · `earnings` · `catalysts` · `thesis` · `morning-note` · `idea-screen` · `sector-overview` | 缓存型（dim 21） |
| [`ic-memo`](../../commands/ic-memo.md) · [`dd`](../../commands/dd.md) | `ic-memo` · `dd` · `competitive` · `unit-economics` · `value-creation` | 缓存型（dim 22） |
| [`ai-readiness`](../../commands/ai-readiness.md) · [`earnings-preview`](../../commands/earnings-preview.md) · [`model-update`](../../commands/model-update.md) | `ai-readiness` · `earnings-preview` · `model-update` | 按需计算 |
| [`rebalance`](../../commands/rebalance.md) · [`returns`](../../commands/returns.md) | `uzi --portfolio <csv> --method rebalance\|returns` | 组合 |

**缓存型**只是读取 `raw_data.json → dimensions.<dim>.data.<key>`（Task 1.5 已算完）；
**按需计算**由 `uzi_models::tier1::*` 现场算。找不到 `--method` 名字时看
[`uzi --help`](../../SKILL.md) 或 `commands/`。

## 🎯 角色定位（非常重要）

- **你不是脚本的搬运工** — 不要只把缓存 JSON 的结果往报告里贴。
- **你是分析师** — 你读原始数据 + 量化结果，然后用自己的判断串起一个有冲突感、有洞察的叙事。
- **脚本提供 5 类产物**：
  1. **原始数据**（Task 1 · 22 维 fetcher）
  2. **机构建模结果**（Task 1.5 · DCF/Comps/LBO/3-Stmt/IC Memo/Porter 等）
  3. **66 人评委量化裁决**（Task 3 · 每人引用具体规则）
  4. **数据完整性报告**（哪些字段缺失 / 哪些降级）
  5. **可审计的 methodology_log**（每一步计算的推导链）
- **你必须在 Task 2 和 Task 4 做真正的定性判断**。

## 🖥 入口 · `uzi` CLI

```bash
cargo build --release -p uzi-cli      # 产物: target/release/uzi
```

| 命令 | 用途 |
|---|---|
| `uzi <ticker>` | 一把跑完 Task 1→5（**快速模式**，评委为规则引擎机械输出） |
| `uzi <ticker> --stage1` | 只跑 Task 1→3，停下等你介入 |
| `uzi <ticker> --stage2` | 合并 `agent_analysis.json` 后跑 Task 4→5 出报告 |
| `uzi <ticker> --school F` | 锁定单一流派视角（A–I） |
| `uzi <ticker> --depth lite\|medium\|deep` | 思考深度（默认 `medium`） |
| `uzi <ticker> --from-modeling` | 复用已缓存的 `raw_data.json` 重跑建模+评分+报告 |
| `uzi <ticker> --no-resume` | 忽略缓存，全量重抓 |
| `uzi <ticker> --force-name 000582.SZ` | 跳过名称解析直接用代码 |
| `uzi --versus A B [C D]` | 2–4 只横向对比 |
| `uzi --portfolio holdings.csv` | 组合批量分析 |
| `uzi --screen daily` | A+港股每日全市场筛选 |
| `uzi <ticker> --remote` | 起 Cloudflare Tunnel 出公网链接 |
| `uzi <ticker> --no-browser --output-dir DIR` | 不开浏览器 / 额外导出产物 |

`<ticker>` 接受代码或中文名（`600519.SH` / `AAPL` / `贵州茅台`）。中文名走
「MX API → 精确子串 → 本地模糊匹配」三级解析；歧义时返回候选清单而非瞎猜。

## ⛔ 硬性门控规则（违反即停止）

1. **必须按 Task 1 → 1.5 → 2 → 3 → 4 → 5 顺序**。前一 Task 的产物 JSON 不存在时禁止开始下一步。
2. **数据必须来自脚本或真实检索**，禁止编造数字。任何推断都要标注来源。
3. **Task 5 禁止空泛话术**（"基本面良好" / "前景广阔" / "值得关注" 出现即失败）。
   必须是有冲突感的定量金句：
   - ✅ "DCF 说高估 28%，但 LBO 说 PE 买方仍赚 21% IRR — 这个分歧值得琢磨"
   - ❌ "估值合理，基本面良好"
4. **矛盾必须呈现，不准和稀泥**：DCF 与 Comps 冲突时把冲突写进报告；66 评委分歧大时，
   **分歧本身是信息**。

### HARD-GATE-UPDATE-PROMPT · 新版本提示

会话启动 hook 会后台跑 `uzi --update-prompt-file .cache/_global/update_prompt.md`：有新版本时
写入提示文案，没有时**删除**该文件（所以文件存在 = 需要提示）。

**你必须在第一次回应用户前检查这个文件**：

1. 读 `.cache/_global/update_prompt.md`（不存在 = 无需提示，跳过）
2. 若存在，把文件完整内容作为**第一条消息**展示给用户（不要改短、不要合并进别的消息）
3. 收集回答，然后落盘：`uzi --update-answer <y|s|n> <version>`（version 见提示文案）
4. 处理完删除该文件，避免同一会话重复弹
5. 按回答继续：
   - `y` → 告诉用户按 README 的更新命令执行，然后继续其原请求
   - `s` → 已记 skip（记到 `.cache/_global/update_check.json` 的 `skipped_version`），继续原请求
   - `n` → 继续原请求

> 用户没有原请求时（刚进会话），展示完提示后等待用户开口。
> 环境变量 `UZI_NO_UPDATE_CHECK=1` 可整体关闭该检查（hook 也会跳过）。

### HARD-GATE-NAME · 股票名纠错

若 `--stage1` 输出 `status: "name_not_resolved"` 或 `status: "non_stock_security"`，
**绝不能**假装猜到正确股票继续跑。用候选清单向用户确认后，用**选定的代码**重跑。
候选为空时告知用户并建议直接输入代码。

### HARD-GATE-NON-STOCK · ETF/LOF/可转债 引导到成分股

`status: "non_stock_security"` 的 payload 含 `security_type` / `label` / `top_holdings`。
66 评委规则全是个股财务指标，ETF/基金/可转债**不该走这个 pipeline**：

1. 明确告知用户"本引擎是个股深度分析，{label} 未覆盖"
2. **ETF**（`top_holdings` 非空）：列出前 10 大持仓，问用户要分析哪只成分股，用**成分股代码**重跑
3. **LOF**：告知基金评估请用专门工具
4. **可转债**：建议分析正股

### HARD-GATE-SCHOOL-LOCK · 用户锁定单一流派

`--school F`（A–I）时 `UZI_SCHOOL` 被设置，`synthesis.json["school_lock"]` 会标注。
进入 role-play 后：

1. **只 role-play `panel.json` 里 `group == UZI_SCHOOL` 的评委**
2. 其他派已被标 `signal="skip"`、`reason="用户锁定 X 派视角"` — 不要给他们写评语
3. `panel_insights` 仅讨论该派**内部分歧**，不写"巴菲特说 X · 赵老哥说 Y"这类跨派对比
4. 该派全看多时，多空辩论也要给**派内分歧版本**（如游资里"打板派 vs 卡位派"）
5. 报告顶部已渲染 SCHOOL LOCK banner

### HARD-GATE-PERSONA · 评委 role-play

评委名册内嵌于 `crates/uzi-investors/src/data/investors.json`（**66 位**，9 大流派）：
A 经典价值 6 · B 成长 9 · C 宏观对冲 7 · D 技术趋势 4 · E 中国价投 7 ·
F 游资 24 · G 量化 4 · H 科技领袖 4 · I Serenity 1。

若 `UZI_PERSONAS_DIR`（默认 `skills/deep-analysis/personas/`，相对当前工作目录）存在 YAML
persona 档案，**flagship persona 的 YAML 优先于规则引擎 headline**：

本仓库已随附 **51 份 persona 档案**（12 个 flagship 手写 + 39 个 stub），从仓库根目录运行时
会被自动加载（`uzi_investors::persona_yaml::personas_dir()`）：

```bash
ls skills/deep-analysis/personas/*.yaml | wc -l   # 51
```

> 注意：`personas_dir()` 是**相对 cwd** 解析的，所以必须从仓库根目录运行 `uzi`，
> 或显式 `export UZI_PERSONAS_DIR=<repo>/skills/deep-analysis/personas`。

- 每条 headline 必须引用 `key_metrics` 里的具体条目（巴菲特说"ROE 连续 10 年 > 15%"、
  段永平说"PE 40 红线"、林奇说"PEG < 1"、赵老哥说"封板时间 + 市值上限"）
- 每条 reasoning 必须带该 persona 的风格词（巴菲特的 "Mr. Market"、林奇的 "tenbagger"、
  木头姐的 "Wright's Law"）
- **signal 必须与其历史立场对齐**：巴菲特不会对 PE 882 的股票说买入；赵老哥不会对
  9000 亿市值说"打板"
- 无名册档案的 persona 由台词库 + 规则引擎驱动，按公开言论风格演绎，**不得编造具体历史言论**

> ⚠️ **三个 flagship 档案的判据字段名不同**（上游同样如此，其 `load_persona` 也只读
> `key_metrics`）：
>
> | persona | 判据字段 |
> |---|---|
> | `dalio` | `key_framework` |
> | `soros` | `key_signals` |
> | `fisher` | 无判据列表 → 用 `famous_positions` + `philosophy` |
>
> 这三人的 `key_metrics` 解析后为空。要按 YAML 严谨 role-play 他们时**读原始 YAML**
> （`Persona::raw` 保留整份解析结果）。

### HARD-GATE-QUALITATIVE · 6 维定性维度必须 agent 深度分析

`3_macro / 7_industry / 8_materials / 9_futures / 13_policy / 15_events` 这 6 维必须由 agent
做跨域联想 + 多源检索后产出结构化分析，不得直接把爬虫片段当评语。

1. 建议 spawn 3 个并行 sub-agent：**Macro-Policy**(3+13) / **Industry-Events**(7+15) /
   **Cost-Transmission**(8+9)
2. 每个都要用真实检索（WebSearch / 浏览器抓原文 / MX API 若 `MX_APIKEY` 已设）
3. 合并写入 `agent_analysis.json` 的 `qualitative_deep_dive`
4. 质量红线：每维 `evidence` ≥ 2 条且每条带具体 URL；6 维合计 `associations` ≥ 3 条

### HARD-GATE-BROWSER-FALLBACK · 浏览器兜底必须真的跑过

用户反馈："我使用下来，并没有遇到模型主动使用 Playwright 的问题"。

`--stage1` 末尾会自动跑一次浏览器兜底（仅对 profile 白名单里的维度、且数据为空或
有效字段 < 50% 时）。**但如果 Stage 1 那一刻某维度 data 非空却全是 "—"，它会被判为
"不需要兜底"而跳过** —— 你介入后往往更清楚哪些维度不够（`_review_issues.json`），
这时必须主动再跑一次：

```bash
UZI_PLAYWRIGHT_FORCE=1 uzi <ticker> --depth deep --stage1   # FORCE=1 覆盖"数据已足"判定
```

时序细节：
- `--depth lite` → `playwright_mode=off`，此门控自动跳过
- `--depth medium` → opt-in，需 `export UZI_PLAYWRIGHT_ENABLE=1`
- `--depth deep` → 默认启用
- 浏览器来自**系统已装的 Chromium 系**（Chrome / Chromium / Edge / Brave / Chrome for
  Testing），通过 CDP 驱动；`uzi --browser-check` 可确认是否找到
- 抓不到时**不能**在评语里写空话：转 WebSearch / MX API，最后降级到"基于公开信息推断，
  非一手"并显式标注

### HARD-GATE-SELF-REVIEW · 机械级自查必须通过才能出 HTML

`--stage2` 在生成 HTML 前跑机械自查；有 critical 会阻止出报告，warning 会记录后继续。
仅调试时可用 `UZI_SKIP_REVIEW=1` 强制跳过。

### HARD-GATE-DATAGAPS · 数据缺口 agent 必须接管

Stage 1 检测到缺口会写 `.cache/{ticker}/_data_gaps.json`。对其中每个字段你都要尝试补齐
（浏览器 → MX API → WebSearch → 逻辑推导）；确实拿不到的，在 `agent_analysis.json` 的
`data_gap_acknowledged` 里显式标注。报告会对这些字段显示 ⚠️ 橙色徽章而非假数据。

## 📋 6 Task 概览

| Task | 名称 | 产物 | 角色 |
|---|---|---|---|
| 0 | 识别股票 | — | 🤖 `uzi` |
| 1 | 22 维数据采集 | `.cache/{ticker}/raw_data.json` | 🤖 脚本 |
| 1.5 | 机构建模（Dims 20–22） | `raw_data.json["dimensions"]["20_valuation_models"]` 等 | 🤖 脚本 + **🧠 假设审查** |
| 2 | 22 维打分 + **定性判断** | `.cache/{ticker}/dimensions.json` | 🤖 脚本 + **🧠 你写评语** |
| 3 | 66 评委量化裁决 | `.cache/{ticker}/panel.json` | 🤖 规则引擎 |
| 4 | 综合研判 + **叙事合成** | `.cache/{ticker}/synthesis.json` | **🧠 你主导** |
| 5 | 报告组装 | `reports/{ticker}_{YYYYMMDD}/full-report.html` | 🤖 脚本 + **🧠 你的金句** |

维度索引：**0–19** 为数据/评分维度（`0_basic` … `19_contests`），**20–22** 为机构建模维度
（`20_valuation_models` / `21_research_workflow` / `22_deep_methods`）。

## ⚡ 两段式执行（数据靠脚本，判断靠你）

### Stage 1 · 数据 + 骨架分

```bash
uzi <股票名或代码> --stage1
```

`--stage1` 自动完成 Task 1 → 1.5 → 2 → 3，打印产物清单与下一步提示，然后**停下**。

### 你的分析环节（Stage 1 之后、Stage 2 之前）

读 `panel.json` 的评委骨架分 → role-play 投资者 → 用你的判断覆盖
`headline` / `reasoning` / `score` → **写 `agent_analysis.json`**（闭环的关键）。

### Stage 2 · 生成报告

```bash
uzi <ticker> --stage2
```

Stage 2 读取你更新后的 `panel.json` + `agent_analysis.json`，合并生成 HTML 报告。
没有 `agent_analysis.json` 时退化为纯脚本模式（会打印警告）。

> ⚠️ `--depth deep` **不属于快速模式**。deep 档必须由你介入 role-play 并写
> `agent_analysis.json`。只有 lite/medium 才适合 `uzi <ticker>` 一把梭。

## 🧠 `agent_analysis.json` 契约

写入 `.cache/{ticker}/agent_analysis.json`。`--stage2` 会用下列规则校验
（`crates/uzi-review/src/validator.rs`），**有 error 会回退到脚本骨架并写
`_agent_analysis_errors.json`**：

> **`analysis_input_hash` 必须来自 `.cache/{ticker}/_agent_review_context.json`**（Stage 1 写入）。
> 它是「你分析的是哪一版数据」的指纹：
> - **`--depth deep` 下必填** —— 缺失会被直接拒绝（"deep 档必须提供 analysis_input_hash"）
> - 填错（与当前 `raw_data.json` 不符）→ 判为过期，**不会被复用**
> - 非 deep 档可省略，此时退回按文件时间戳比较（分析早于 raw_data 即拒绝）
>
> ```bash
> # 取指纹（jq 缺失时用 cat 直接看）
> jq -r .analysis_input_hash .cache/<ticker>/_agent_review_context.json
> ```

| 字段 | 要求 | 违反 |
|---|---|---|
| `agent_reviewed` | 必须 `true` | 缺失/非 true → 整体拒绝复用 |
| `analysis_input_hash` | 见上；deep 档必填 | deep 缺失 → 拒绝；不符 → 视为过期 |
| `per_investor_override` | dict：`{investor_id: {signal, score, headline, reasoning, comment, verdict}}`。这是**逐人覆盖**的唯一入口 —— 你 role-play 的结论写这里，**不需要手改 `panel.json`** | 未知 `investor_id` 会被静默跳过；只覆盖列出的字段 |
| `dim_commentary` | dict，key 为维度名；**每条 ≥ 20 字**，引用具体数字 | 非 dict/非 string → error；< 20 字 → warning |
| `panel_insights` | **≥ 30 字**：投票分布 + 多空分歧 | warning |
| `great_divide_override` | `punchline`(≥10 字) + `bull_say_rounds`(≥3 条) + `bear_say_rounds`(≥3 条) | 非 dict → error；条数不足 → error |
| `narrative_override.core_conclusion` | **≥ 20 字**综合定论 | warning |
| `narrative_override.risks` | **≥ 3 条** | warning |
| `narrative_override.buy_zones` | 必须含 `value` / `growth` / `technical` / `youzi` 四个 key，各含 `price` + `rationale`(≥5 字) | 缺 key → error；缺子字段 → warning |
| `qualitative_deep_dive` | 覆盖 6 维；每维 `evidence`(≥2 条 `{source,url,finding,retrieved_at}`) + `associations`(6 维合计 ≥3 条) + `conclusion` | `evidence` 非 list → error；`associations` < 3 → warning |
| `data_gap_acknowledged` | dict `{dim_key: "已尝试 X 但失败的原因"}` | 非 dict → error |

> `dim_commentary` 覆盖维度不足也会被自查记 warning。deep 档建议覆盖全部 22 维，
> 至少不要漏掉 `14_moat` / `13_policy` / `7_industry` 这些定性维度。

```json
{
  "agent_reviewed": true,
  "analysis_input_hash": "<取自 _agent_review_context.json>",
  "per_investor_override": {
    "buffett": {
      "signal": "bullish",
      "score": 88,
      "headline": "ROE 32.5%、负债率 15%，这是我能看懂的生意。",
      "reasoning": "十年后这家公司大概率还在卖同样的酒，且还能提价。"
    },
    "zhao_lg": {
      "signal": "skip",
      "score": 0,
      "headline": "市值 1.59 万亿，不在我射程内。"
    }
  },
  "dim_commentary": {
    "0_basic": "建筑央企，主营市政/房建。市值偏小，营收稳但利润率极薄（1.2%），典型低毛利基建股。",
    "1_financials": "ROE 不到 8%，连续 3 年下滑。现金流波动大，应收账款占营收比偏高，回款风险明显。",
    "2_kline": "均线空头排列，MACD 死叉，量能萎缩。典型下跌趋势，不满足 Stage 2 条件。"
  },
  "panel_insights": "66 评委中，价值派集体看空（ROE 太低+无护城河），游资中性（有地方城投概念但板块热度不够），只有少数逆向投资者给出中性偏多。整体共识 32%，偏弱。",
  "great_divide_override": {
    "punchline": "DCF 说高估 23%，但城投重组预期让 LBO 视角的 IRR 仍有 18% — 这个冲突值得关注。",
    "bull_say_rounds": ["宁波城投整合预期 + 地方债化解受益", "PB 仅 0.9x，历史底部区间", "综合看 62 分"],
    "bear_say_rounds": ["ROE 连降 3 年，毛利率是天花板", "应收账款/营收 > 60%", "综合看 35 分"]
  },
  "narrative_override": {
    "core_conclusion": "宁波建工 · 48 分 · 谨慎。典型地方基建股，ROE 不到 8%、毛利率 8%，靠城投整合讲故事。DCF 高估 23%，但 LBO 压力测试 IRR 18% — 博弈价值存在但风险更大。",
    "risks": ["ROE 持续下滑", "应收账款占比过高", "地方财政压力传导"],
    "buy_zones": {
      "value": {"price": 3.85, "rationale": "PB 0.8x · 历史底部 + 净资产折价"},
      "growth": {"price": 4.10, "rationale": "城投整合落地前的博弈价"},
      "technical": {"price": 4.25, "rationale": "MA120 支撑位 · 需放量确认"},
      "youzi": {"price": 4.50, "rationale": "城投板块联动时的短线切入点"}
    }
  }
}
```

Agent 写入的字段优先级高于脚本生成的 stub。若 `agent_analysis.json` 与当前
`raw_data.json` 指纹不匹配，它会被判为过期而**不被复用**（重新跑 `--stage1` 或更新分析）。

## 🎛 档位与环境变量

三档 `AnalysisProfile` 决定启用哪些 fetcher、多少评委投票、自查多严：

| 变量 | 作用 |
|---|---|
| `UZI_DEPTH` | `lite` / `medium` / `deep`（或 `--depth`） |
| `UZI_SCHOOL` | 锁定流派 A–I（或 `--school`） |
| `UZI_SKIP_REVIEW` | `1` = 跳过自查门控（**仅调试**） |
| `UZI_NO_RESUME` | `1` = 忽略缓存全量重抓 |
| `UZI_CACHE_ROOT` | 覆盖 `.cache` 根目录 |
| `UZI_REPORTS_DIR` / `UZI_REPORTS_ROOT` | 覆盖报告输出目录 |
| `UZI_ASSETS_DIR` | 覆盖 `assets/`（模板、头像） |
| `UZI_HTTP_TIMEOUT` | 单次 HTTP 超时秒数 |
| `UZI_PLAYWRIGHT_ENABLE` | `medium` 档启用浏览器兜底 |
| `UZI_PLAYWRIGHT_FORCE` | `1` = 对所有白名单维度强制跑兜底 |
| `UZI_XQ_LOGIN` | `1` = 启用雪球登录态抓取（或 `--enable-xueqiu-login`） |
| `UZI_SKIP_PREFLIGHT` | `1` = 跳过网络预检 |
| `MX_APIKEY` | 设置后 MX 妙想 API 参与解析与补齐 |

## 🛠 维护 / 排障命令

```bash
uzi --preview          # 用内置 mock 数据离线生成完整报告（验证模板，不联网）
uzi --prewarm          # 预热跨股公共缓存（A 股名称表 + 宏观/政策/护城河检索）
uzi --check-update     # 检查新版本后退出（--force 语义）
uzi --xueqiu-status    # 雪球登录状态
uzi --xueqiu-login     # 一次性交互式雪球登录（保存 cookie 供后续复用）
uzi --browser-check    # 探测可用浏览器（CDP 兜底依赖）
```

## 📁 数据契约（缓存路径）

```
.cache/{ticker}/
  raw_data.json                  # Task 1 + 1.5：22 维 + 建模维度
  dimensions.json                # Task 2：22 维评分
  panel.json                     # Task 3：66 评委 Signal
  synthesis.json                 # Task 4：综合研判
  agent_analysis.json            # 你写的定性分析（闭环关键）
  _agent_review_context.json     # Stage 1 写入：analysis_input_hash 指纹 + depth
  _data_gaps.json                # 采集缺口 + 恢复任务
  _review_issues.json            # 自查 issue（含低质量维度清单）
  _agent_analysis_errors.json    # agent_analysis 校验失败明细
  _pipeline_fallback.json        # 降级/兜底记录
reports/{ticker}_{YYYYMMDD}/
  full-report.html               # 完整报告
  full-report-standalone.html    # 单文件（内联资源，可分享）
  one-liner.txt                  # 一句话结论
  avatars/                       # 评委头像
```

`raw_data.json` 的形状与上游一致：维度挂在 **`raw["dimensions"]["<dim_key>"]`** 下，
`ticker` / `fund_managers` / `similar_stocks` 在顶层。
每个维度是 `{data, source, fallback, _pipeline}`。

## 🧪 开发与验证

```bash
cargo test --workspace                    # 全量测试（含对照上游 Python 的 golden 差分）
cargo build --workspace --all-targets     # 期望 0 warning
cargo run -p uzi-data --example fetch_one -- 600519.SH   # 数据层冒烟：打印 0_basic
uzi --preview                             # 报告层冒烟：离线出 mock 报告
```

Golden 差分测试在 `tools/golden/`：`dump_*.py` 跑上游 Python 产出 `expected/<case>/*.json`，
Rust 侧用 `uzi_core::testkit::assert_json_eq` 逐键比对（键序 + 浮点精确相等）。
新增可差分行为时，同步补 `dump_*.py` 与对应 `crates/*/tests/*.rs`。

文档侧另有一道守卫：改了任何 `SKILL.md` / `references/*.md` 后跑

```bash
python3 tools/skills/verify_docs.py
```

它校验文档里每个 `uzi_*::…` 路径真实存在、每个相对链接可解析、且没有残留可执行的
Python 调用（本项目是 Rust 二进制，不该让读者去执行 Python）。详见
[`tools/skills/README.md`](../../tools/skills/README.md)。

## 📖 文档结构

```
SKILL.md                          根索引：4 个 skill 的选择表
skills/deep-analysis/SKILL.md     工作流总纲 + references/ 手册索引
skills/deep-analysis/references/  分任务操作手册（task1…task5、data-sources、fin-methods/）
skills/investor-panel/SKILL.md    评审团 + references/ 流派方法论与语料库
skills/lhb-analyzer/SKILL.md      龙虎榜 + references/ 席位百科
skills/trap-detector/SKILL.md     杀猪盘 + references/ 八信号详解
```

`references/` 是从上游迁入并**逐处改写为 Rust 口径**的操作手册 —— 引用的是真实
Rust 模块路径与 `uzi` 命令，不是上游的 Python 模块名。

**现在开始**：从第 0 步识别股票开始。记住 — **你是分析师，不是脚本运行器。**
