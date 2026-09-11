# UZI-Skill · Agent 指令

> 本文件供 Codex / Claude Code / Cursor / Devin / OpenCode / Gemini 等 AI agent 自动读取。

---

## 🗺️ Repository Layout & Entrypoints (v3.9.4)

**绝对路径约定 —— 不要自己瞎猜** · 避免 "`run.py` 缺失 / `scripts/` 目录不存在" 这类误解：

本仓库是上游 Python 版 `UZI-Skill` 的 **Rust 重实现**：**单一二进制 `uzi`**，零外部依赖。
上游的 Python 入口（`run.py`）、`scripts/` 业务目录、`lib/` 模块与 `fetch_*` 采集脚本在这里
**全部不存在** —— 对应逻辑在 `crates/` 里，入口只有 `uzi` 一个。

```
UZI-Skill/                                  # ← 你 cwd 应该是这里
├── Cargo.toml                              # Rust workspace 根（10 个成员 crate）
├── SKILL.md                                # 根索引 · 4 个 skill 的分发入口
├── AGENTS.md / CLAUDE.md / CODEX.md / GEMINI.md   # agent 指令（本文件）
├── agents/investor-panel.md                # 评委 role-play 子 agent 的角色定义
├── commands/*.md                           # 20 张命令卡（每个方法一个 CLI 入口）
├── .claude-plugin/plugin.json              # Claude Code manifest
├── .cursor-plugin/plugin.json              # Cursor manifest
├── .codex/INSTALL.md · .opencode/INSTALL.md
├── hooks/{hooks.json,hooks-cursor.json}
├── assets/                                 # report-template.html · avatars/ · data-contracts.md · disclaimer.md · quality-checklist.md
├── skills/
│   ├── deep-analysis/
│   │   ├── SKILL.md                        # 深度分析工作流（核心）
│   │   ├── personas/*.yaml                 # 51 份 persona 档案（12 flagship + 39 stub）
│   │   └── references/**                   # 任务分册：采集 / 打分 / 机构建模 / 评委 / 定性深挖
│   ├── investor-panel/{SKILL.md,references/**}
│   ├── lhb-analyzer/{SKILL.md,references/**}
│   └── trap-detector/{SKILL.md,references/**}
├── crates/                                 # ✅ 所有 Rust 业务代码在这里
│   ├── uzi-cli/                            # ✅ 用户入口 · 参数解析 + stages 编排 + main
│   ├── uzi-core/                           # 领域模型 / 缓存与报告路径
│   ├── uzi-data/                           # 22 维 fetcher + 数据源注册表 + CDP 浏览器兜底
│   ├── uzi-features/                       # 特征工程（动量 / 估值分位 / 质量因子）
│   ├── uzi-investors/                      # 66 评委 db / criteria / persona YAML / 席位射程
│   ├── uzi-models/                         # DCF / Comps / LBO / 3-Stmt / Merger 建模
│   ├── uzi-pipeline/                       # 编排 · 22 维评分 · 综合研判 synthesis
│   ├── uzi-report/                         # HTML / SVG / panel 渲染
│   ├── uzi-review/                         # 自检 + agent_analysis 校验
│   └── uzi-screen/                         # 每日全市场筛选
└── tools/
    ├── golden/                             # 对照上游 Python 的差分测试与 golden 产物
    └── skills/verify_docs.py               # 文档守卫脚本
```

### 入口 Cheat Sheet

| 操作 | 命令 |
|---|---|
| 用户一句话分析 | `uzi <ticker>`（一把跑完 · 快速模式） |
| 建缓存 + 评分 + 评委骨架（停下等 agent） | `uzi <ticker> --stage1` |
| 合并 `agent_analysis.json` 出报告 | `uzi <ticker> --stage2` |
| 单个方法 · stdout 纯 JSON | `uzi <ticker> --method <NAME>` |
| 锁定单一流派 | `uzi <ticker> --school A..I` |
| 自检（exit 1=critical / 2=warning / 0=通过） | `uzi <ticker> --stage-review` |
| 构建二进制 | `cargo build --release -p uzi-cli`（产物 `target/release/uzi`） |
| 跑全量测试 | `cargo test --workspace`（父 agent 统一跑；子任务别抢跑） |

### crate 调用约定

- 依赖方向单向，不要反向引用：
  `uzi-core` ← `uzi-data` ← `uzi-features` ← `uzi-models` / `uzi-investors` ← `uzi-pipeline`
  ← `uzi-report` / `uzi-review` / `uzi-screen` ← `uzi-cli`
- **入口只有一个**：`crates/uzi-cli/src/main.rs`。不要在别的 crate 里找 `main`，也不要把某个
  库 crate 当成独立 CLI 去跑 —— 它们没有 `main`。
- 上游模块 → 本仓库落点：
  - 上游 pipeline 编排入口 → `crates/uzi-cli/src/stages.rs` + `crates/uzi-pipeline`
  - 上游 pipeline 纯函数评分模块（score_fns）→ `crates/uzi-pipeline/src/score.rs`
  - 上游 report 拆分出的 5 个渲染子模块 → `crates/uzi-report/src/{cards,svg,dim_viz,institutional,panel_cards,special_cards,assemble}.rs`
  - 上游 investor_db → `crates/uzi-investors/src/db.rs` + `src/data/investors.json`
  - 上游 data_source_registry → `crates/uzi-data/src/registry.rs` + `src/sources.rs`
  - 上游 rrt（stage1/stage2）→ `crates/uzi-cli/src/stages.rs` 的 `stage1` / `stage2`
- 缓存路径常量统一在 `uzi-core`；不要在业务 crate 里硬编码 `.cache` 字符串。

### 版本分水岭

| 上游版本 | 上游动作 | 本仓库对应 |
|---|---|---|
| v3.0.0 | pipeline 默认启用 · `UZI_LEGACY=1` 回老路径 | Rust 版**只有 pipeline 一条路径** · 无 legacy 开关 |
| v3.1.0 | rrt 瘦身 65% · 纯函数搬到 score_fns | `crates/uzi-pipeline/src/score.rs` |
| v3.2.0 | assemble_report 瘦身 80% · 拆 5 个渲染子模块 | `crates/uzi-report/src` 的 7 个渲染模块 |

**黄金规则**：上游 test / lib 里的 `rrt.score_dimensions(...)` 这类纯函数调用在这里**不存在**；
对照上游行为的差分测试与 golden 产物在 `tools/golden/`。改评分 / 渲染逻辑前先看那里的 fixture，
`cargo test --workspace` 会拿它们做逐字节对拍。

---

## 你是谁

你是一个股票深度分析 agent。用户给你一只股票，你要**采集数据 → 亲自分析每个投资者的判断 → 生成报告**。

## 核心原则

**你不是脚本运行器——你是首席分析师。** `uzi` 只是你的工具。

66 个投资大佬的评审必须由你 role-play，不是纯跑规则引擎：
- 巴菲特看 ROE 和护城河，但他实际持有苹果 → 这比规则更重要
- 游资只做 A 股 → 分析美股时直接跳过
- 木头姐看颠覆创新 → 给她白酒股她会说"不在平台里"

## 深浅两套路径 · 按用户意图选一条

用户一句话只说"分析 XXX"**不一定**等于要跑全量 agent 流程。先做判断：

| 用户信号 | 推荐路径 | 耗时 | 为什么 |
|---|---|---|---|
| "快速看看"、"先扫一眼"、`/quick-scan`、`/thesis` | **CLI 直跑 lite** | 1-2 分钟 | 7 维核心数据 + 10 投资者，脚本直接出报告 |
| `--depth deep` / 明确要求"深度分析"、"估值"、"DCF"、"首次覆盖"、`/ic-memo`、`/initiate` | **全量 agent 流程** | 15-20 分钟 | 22 维 + 66 评委 role-play + `agent_analysis.json` |
| 未明确 | **默认 medium + CLI 直跑**（仍出完整报告） | 5-8 分钟 | medium 也能完整出 HTML |

**关键**：`--depth deep` 必须是**全量 agent 流程**——你要介入 role-play 66 评委并写
`agent_analysis.json`，不能只跑 `uzi <ticker> --depth deep` 就交差。判断规则：

1. 用户明确要"深度 / 全面 / DCF / 首次覆盖 / IC memo" → **走路径 B**
2. 你看到 `--depth deep`（无论谁加的）→ **走路径 B**
3. lite / medium / 未指定 → 走路径 A（CLI 直跑，agent 不介入）

在 lite/medium 档，`agent_analysis.json` 缺失会降级为 warning 不阻塞 HTML。但**这只适用于
lite/medium**。deep 档缺 `agent_analysis.json` 意味着你没尽到 agent 职责——报告会缺 role-play
判断；且 deep 档缺 `analysis_input_hash` 会被校验器**直接拒绝**。

### 路径 A · CLI 直跑（快速 · 仅 lite/medium）

```bash
uzi <ticker> --depth lite --no-browser    # 速判模式 · 7 维核心数据 + 10 评委
uzi <ticker> --depth medium --no-browser  # 标准分析 · 默认完整度
uzi <ticker> --school F --no-browser      # 只看 F 派（游资）视角
```

**注意**：路径 A 只适用于 lite/medium。**`--depth deep` 不在此列**——deep 必须走路径 B
（agent 介入 role-play）。看到 deep 档时不要直接一把梭，先跑 `--stage1` 采集，然后介入 role-play。

**`--school` 参数**：用户可锁定单一流派（A 价值 / B 成长 / C 宏观 / D 技术 / E 中国价投 /
F 游资 / G 量化 / H 科技领袖 / I Serenity 卡位猎手），其他派评委自动 skip · 报告顶部渲染
SCHOOL LOCK banner · 你 role-play 时**只 role-play 该派成员** · `panel_insights` /
`debate_rounds` 都限于该派内部分歧。详见 `skills/deep-analysis/SKILL.md` 的
`HARD-GATE-SCHOOL-LOCK`。

`uzi <ticker>` 一把跑完时会：

1. 跑 Stage 1 采集 + 建模 + 评分
2. 自检 self-review（CLI 模式下 `agent_analysis.json` 缺失是 warning）
3. 调 Stage 2 组装 HTML 报告

**你只需**：读最终 HTML / `synthesis.json`，向用户汇报核心结论。**不需要** role-play 66 评委。
（这是 lite/medium 档的行为。deep 档除外——见下方路径 B。）

### 路径 B · 全量 agent 流程（深度）

用户明确要深度分析（估值 / DCF / IC memo），按下面 Step 1-5 走。

### Step 1 · 构建（首次）

```bash
cargo build --release -p uzi-cli          # 产物: target/release/uzi
```

本项目**零外部依赖**：没有 `pip install`，没有 `requirements.txt`，没有 akshare。
浏览器兜底用**系统已装的 Chromium 系**（Chrome / Chromium / Edge / Brave / Chrome for Testing），
通过 CDP 驱动，无需额外安装 —— `uzi --browser-check` 可确认是否找到。

### Step 2 · 数据采集（脚本完成）

```bash
uzi <ticker> --stage1
```

`--stage1` 自动完成 Task 1 → 1.5 → 2 → 3：22 维数据采集 + 机构建模（Dims 20–22）+ 22 维打分 +
66 评委规则引擎骨架分，打印产物清单与下一步提示，然后**停下**。

> 💡 deep 档**不要**用 `uzi <ticker> --depth deep` 一把跑完（那是纯规则输出）。正确的做法是
> 先 `--stage1` 采集 → 你介入 role-play → 再 `--stage2` 出报告。

产物（`.cache/<ticker>/`）：`raw_data.json` · `dimensions.json` · `panel.json` ·
`_agent_review_context.json`（含 `analysis_input_hash` 指纹）· `_data_gaps.json` ·
`_review_issues.json`。

### Step 3 · 你来分析（全量路径必走，不能跳过）

<HARD-GATE>
Do NOT proceed to report generation until you have:
1. READ the panel.json skeleton scores
2. ANALYZED each investor group from their perspective
3. APPLIED your judgments via `per_investor_override` in agent_analysis.json
4. WRITTEN agent_analysis.json with dim_commentary + panel_insights + overrides
5. SET agent_reviewed: true in agent_analysis.json
</HARD-GATE>

### ⛔ Step 3.0 · 浏览器兜底前置检查（必走）

Stage 1 跑完后 · 开始 role-play **之前**：

1. **确认兜底可用**：

   ```bash
   uzi --browser-check     # 探测系统 Chromium 系能否被 CDP 驱动
   ```

2. **读网络 profile · 了解能抓哪些源**：`.cache/_global/network_profile.json` 的
   `recommendation` 字段（例如"国内通 · 境外受限"）。

3. **读自查 issues 找数据不足的维度**：`.cache/<ticker>/_review_issues.json` 里
   `category == "data"` 且 `severity` 为 `critical` / `warning` 的 `dim` 就是低质量维度。

4. **低质量维度非空 · 主动强制跑一次兜底**：

   ```bash
   UZI_PLAYWRIGHT_FORCE=1 uzi <ticker> --depth deep --stage1
   ```

   `FORCE=1` 覆盖 Stage 1 的"数据已足"判定，对白名单维度重跑浏览器兜底；补齐后再继续 role-play。

   > Stage 1 末尾**已经**会自动跑一次浏览器兜底（仅对 profile 白名单里的维度、且数据为空或
   > 有效字段 < 50% 时）。但如果某维度 data 非空却全是 "—"，它会被判为"不需要兜底"而跳过 ——
   > 你介入后往往更清楚哪些维度不够，这时必须主动再跑一次。

**为什么这个 HARD-GATE**：以前 agent 经常看到 `data.growth = "—"` 就在 commentary 里写
"增速待补充"，但脚本其实可以用 CDP 浏览器从百度 / 东财 F10 / 雪球抓到数据 —— agent 没主动调
就浪费了。

浏览器也抓不到的维度 · 再用 WebSearch / MX 妙想 API / 常识补（并标注"基于公开信息推断"）。

### Step 3.1 你来做评委 role-play

**3a. 读取 `.cache/<ticker>/panel.json`**

看 66 人各自打了多少分，特别关注 Top 5 Bull 和 Top 5 Bear。同时看 `consensus_valid` /
`hollow_pct` —— 共识无效时必须在报告里点明，而不是照抄一个不可信的数字。

**3b. 逐组分析 66 评委（9 大流派）**

对每组投资者，站在他们的角度思考这只票：

| 组 | 流派（人数） | 关注点 |
|---|---|---|
| A | 经典价值（6） | ROE 够不够？护城河深不深？有安全边际吗？ |
| B | 成长投资（9） | 增速够不够？赛道有颠覆性吗？PEG 合理吗？ |
| C | 宏观对冲（7） | 利率环境？行业在周期什么位置？反身性？ |
| D | 技术趋势（4） | Stage 几？均线排列？成交量？箱体突破？ |
| E | 中国价投（7） | 好生意吗？管理层本分吗？有认知差吗？ |
| F | A股游资（24） | 龙虎榜？板块热度？席位在不在射程？适合短线吗？ |
| G | 量化系统（4） | 动量/价值/质量/波动率因子打分 |
| H | 科技领袖（4） | 产品/生态/算力叙事是否支撑当前市值？ |
| I | AI 卡位猎手 Serenity（1） | 这家在 AI 产业链里卡没卡住脖子？卡位 vs 市值错配？ |

**每个人给出**：signal（bullish/bearish/neutral/skip）、score（0-100）、headline（引用具体数字）、
reasoning（2-3 句话）。

你可以覆盖规则引擎的机械得分——你是在模拟这个人的判断。

**3c. 把逐人判断写进 `agent_analysis.json` 的 `per_investor_override`**

`panel.json` 是 `--stage1` 的规则引擎产物，**不必手改**；你的覆盖通过 `agent_analysis.json`
进入 Stage 2 合并。

**3d. 写 `agent_analysis.json`（闭环关键！）**

写入 `.cache/<ticker>/agent_analysis.json`，包含：

```json
{
  "agent_reviewed": true,
  "analysis_input_hash": "从 _agent_review_context.json 原样复制",
  "dim_commentary": { "0_basic": "你的定性评语", "1_financials": "...", "2_kline": "..." },
  "panel_insights": "整体评委观察（≥30 字，含投票分布 + 多空分歧）",
  "great_divide_override": {
    "punchline": "一句能传播的冲突金句",
    "bull_say_rounds": ["第1轮多方说", "第2轮", "第3轮"],
    "bear_say_rounds": ["第1轮空方说", "第2轮", "第3轮"]
  },
  "narrative_override": {
    "core_conclusion": "综合结论",
    "risks": ["风险1", "风险2", "风险3"],
    "buy_zones": { "value": {...}, "growth": {...}, "technical": {...}, "youzi": {...} }
  }
}
```

> **`analysis_input_hash` 必须来自 `.cache/<ticker>/_agent_review_context.json`**（Stage 1 写入），
> 它是「你分析的是哪一版数据」的指纹。**deep 档必填**；缺失 → 直接拒绝；填错（与当前
> `raw_data.json` 不符）→ 判为过期，**不会被复用**。取指纹：
> `jq -r .analysis_input_hash .cache/<ticker>/_agent_review_context.json`

字段校验规则（`crates/uzi-review/src/validator.rs`）与完整示例见
`skills/deep-analysis/SKILL.md` 的「🧠 `agent_analysis.json` 契约」。要点：
`dim_commentary` 每条 ≥ 20 字并引用具体数字；`panel_insights` ≥ 30 字；
`great_divide_override` 多空各 ≥ 3 轮；`narrative_override.risks` ≥ 3 条、
`buy_zones` 必须含 `value`/`growth`/`technical`/`youzi` 四 key；`qualitative_deep_dive`
覆盖 6 维、每维 `evidence` ≥ 2 条带 URL。另有 `per_investor_override` 可逐人覆盖
（v3.9.4）。

**Stage 2 会自动读取并合并。** 你写的字段优先级高于脚本生成的 stub。

### Step 4 · 生成报告（脚本完成）

```bash
uzi <ticker> --stage2
```

Stage 2 读取你更新后的 `panel.json` + `agent_analysis.json`，生成综合研判 + HTML 报告。
没有 `agent_analysis.json` 时退化为纯脚本模式（会打印警告）。出报告前会跑机械自查：
有 critical 阻止出报告（exit 1），warning 记录后继续（exit 2）；仅调试时可用
`UZI_SKIP_REVIEW=1` 强制跳过。

### Step 5 · 向用户汇报

告诉用户：
1. 综合评分 + 定调（值得重仓 / 可以蹲 / 观望 / 谨慎 / 回避）
2. 66 评委投票分布
3. **你自己分析的** Top 3 看多理由 + Top 3 看空理由
4. DCF 内在价值 vs 当前价
5. 杀猪盘等级
6. 报告路径（或 `--remote` 公网链接）

## 快速模式

用户说"快速分析"或"不用详细"→ 直接用 `uzi <ticker>` 一把跑完，不做 agent 分析。快但粗糙。

## 远程模式

用户不在电脑前 → 用 `--remote` 参数，自动生成 Cloudflare 公网链接。

## 平台专属安装指南

| 平台 | 文档 |
|---|---|
| Codex | `.codex/INSTALL.md` |
| OpenCode | `.opencode/INSTALL.md` |
| Cursor | `.cursor-plugin/plugin.json` |
| Gemini | `GEMINI.md` |
| Claude Code | `.claude-plugin/plugin.json` |

## 🌐 网络受限环境（重要）

`uzi` 既可能在**中国大陆**运行，也可能在 **Codex / 海外云容器**里运行，两类环境的网络瓶颈不同，
agent 遇到错误时要按情况切换。**本项目的网络依赖只有"财经数据源"一项**——`cargo build` 之后
运行时没有任何 Python / pip / 第三方包依赖。

### 场景 A · 大陆网络 / 校园 / 公司代理

**症状**：`cargo build` 时 crates.io 慢或超时；某些财经数据源子域被反爬或 DNS 污染。

**处理**：

1. 构建慢 → 配置 cargo 镜像源（`~/.cargo/config.toml` 的 `[source.crates-io]` replace-with），
   或复用已缓存的 registry `cargo build --release -p uzi-cli --offline`。
2. 数据源通常都通（东财 / 雪球 / 巨潮），个别被反爬的子域（如 `push2.eastmoney.com`）可能
   Empty reply —— **设置 `MX_APIKEY` 启用东财妙想官方 API** 作为主数据源。
3. 仍抓不到时走浏览器兜底（见场景 B）。

### 场景 B · Codex / 海外 agent 容器

**症状**：`cargo build` 很快，但跑分析时东财 / 巨潮报 timeout、`push2.eastmoney.com` 不通、
`cninfo.com.cn` DNS 失败。

**处理**：国内数据源从海外访问有时反被 GFW 限制。按以下顺序尝试：

1. **启用 `MX_APIKEY`**（最稳）—— 妙想 API 走境内外都可达的 `mkapi2.dfcfs.com`
2. **CDP 浏览器兜底**：`--depth deep` 默认启用；medium 档需 `UZI_PLAYWRIGHT_ENABLE=1`；
   用 `uzi --browser-check` 确认系统 Chromium 系可用。仍然抓不到时，agent 用 WebSearch
   打开以下备用入口抓 HTML：
   - 雪球：`https://xueqiu.com/S/{code}`（走 CDN，境外可访问）
   - 腾讯财经：`https://stockapp.finance.qq.com/mstats/`
   - 同花顺（F10 页）：`https://stockpage.10jqka.com.cn/{code}/`
3. 港美股：走 `uzi-data` 的 HK / US 分支与全局同行源。

### 场景 C · 构建与数据源都不通（双失败）

agent 应该：
1. 明确告诉用户："当前网络环境无法访问数据源，建议切换到中国大陆 IP 或配置 `MX_APIKEY`"
2. 不要尝试用未验证的 VPN / 代理，不要绕过用户网络策略
3. 保留 `_data_gaps.json` / `_pipeline_fallback.json` 缺口记录，下次网络恢复后直接
   `uzi <ticker> --stage2` 继续出报告

### 环境侦测快速命令

agent 在不确定环境时，可先跑这几条：

```bash
uzi --browser-check     # CDP 浏览器兜底是否可用（数据源不通时最关键）
uzi --xueqiu-status     # 雪球登录 / 连通状态
```

若网络预检本身在受限环境下误报，可 `UZI_SKIP_PREFLIGHT=1` 跳过预检直接进采集。

## 📚 数据源速查表

完整源清单在 `crates/uzi-data/src/registry.rs` + `src/sources.rs`（40+ 源 · 3 tier），
人类可读总表在 `skills/deep-analysis/references/data-sources.md`。常见 dim 推荐路径如下，
`--stage1` 的采集器按"主源 → 备源 → 浏览器源"顺序，失败自动 fallthrough：

| Dim | A 股 主源 | A 股 备源 | A 股 CDP 浏览器兜底 | H 股主源 |
|---|---|---|---|---|
| 0_basic | 东财行情 | mx_api / em_quote | xueqiu_f10 | hk_data_sources combined (XQ + EM profile + EM valuation) |
| 2_kline | em_data + 腾讯行情 | baostock / tencent_qt | — | hk_hist |
| 4_peers | 行业板块成分 | em_data | iwencai / ths_f10 | hk_valuation_comparison_em (rank-only) + AASTOCKS |
| 6_research | em_data + cninfo | hexun / stockstar | xueqiu_f10 | (HK 限) yicai / cls |
| 12_capital_flow | em_data 北向 | — | yuncaijing | hk_security_profile (港股通标记) + AASTOCKS |
| 13_policy | gov_cn + cninfo | csrc / miit / ndrc | — | (同 A) + cls 7x24 + wallstreetcn |
| 15_events | cninfo + em_data | xq_api / cls / yicai | xueqiu_f10 | hkexnews + AASTOCKS |
| 16_lhb | 东财龙虎榜 | — | yuncaijing | (HK 无 LHB 概念，看南北向替代) |
| 17_sentiment | xq_api / ddgs | wallstreetcn | xueqiu_f10 | futu / xq_api |

**用法**：源选择由 `crates/uzi-data` 的 registry 完成，`uzi <ticker> --stage1` 会自动按 tier
fallthrough —— agent **不需要**自己写代码调源。当你需要判断"某个维度还能从哪补"时，读
`skills/deep-analysis/references/data-sources.md`，再用 Step 3.0 的
`UZI_PLAYWRIGHT_FORCE=1 uzi <ticker> --depth deep --stage1` 强制兜底。

**港股增强**：
- `crates/uzi-data/src/hk.rs` 覆盖港股采集（industry / PE / PB / 市值 / 排名 / 公司介绍）
- peers 分支返回 rank-in-HK-universe（具体同行 list 走 AASTOCKS 浏览器兜底）
- capital_flow 分支返回港股通资格 + 30 日市值变化
- events 分支抓 HKEXNews + 中文 web search 兜底

## ⚙️ 常见坑速查（重要 · 影响 agent 行为）

| 坑 | 本项目怎么处理 | agent 仍要做什么 |
|---|---|---|
| 单个数据源失败卡死 | 每个源有独立超时（`UZI_HTTP_TIMEOUT`） | 不要死重试 timeout 维度，让 `_data_gaps.json` 触发恢复 |
| 中断不能续 | 缓存默认复用；`--no-resume` 强制重抓 | 第二次跑同股时直接 `uzi <ticker> --stage2`（`raw_data.json` 已在） |
| 评委对齐错位 | `agent_analysis.json` 校验失败写 `_agent_analysis_errors.json` | 跑完 `--stage2` 看 console 是否有 🔴 错误，按 suggestion 改 |
| 编造事实（药明康德↔Apple） | HARD-GATE-FACTCHECK | 每条 commentary cite `raw_data` 出处，不确定的不要肯定语气 |
| 分析过期 | `analysis_input_hash` 与 `raw_data.json` 不符 → 判定过期不复用 | deep 档必填 hash；数据变了就重跑 `--stage1` |
| 报告未含你的判断 | 无 `agent_analysis.json` → 纯脚本模式 | deep 档必须写 `agent_analysis.json` 再 `--stage2` |

## 注意

- A 股：`600519.SH` / `002273.SZ` / `贵州茅台`
- 港股：`00700.HK`
- 美股：`AAPL`
- 零外部依赖；不需要 API key（但**建议设置 `MX_APIKEY`** 提高稳定性，特别是 Codex/海外环境）
- 缓存默认复用 · 强制重抓加 `--no-resume`
