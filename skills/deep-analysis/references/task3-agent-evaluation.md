# Task 3 · Agent-Driven 评审团 — 66 人每人都是一个决策过程

> **核心原则**：规则引擎（`uzi_investors::criteria` + `uzi_investors::evaluator` + `uzi_pipeline::panel`）是参考材料，不是最终判断。每个投资者的观点必须经过你的"角色扮演式思考"。

## 为什么不能纯规则引擎

| 场景 | 规则引擎给出的 | 正确的答案 |
|---|---|---|
| 巴菲特分析苹果 | ROE pass / PE fail → 62 分中性 | **100 分看多** — 他的第一大持仓 |
| 游资分析美股 | Stage 2 pass → 75 分看多 | **不适合** — 游资不做美股 |
| 木头姐分析白酒 | 营收增速 10% < 20% → 0 分看空 | **不适合** — 她只看颠覆创新 |
| 段永平分析茅台 | ROE pass → 80 分 | **应该更高** — 他实际重仓茅台 |
| 格雷厄姆分析英伟达 | PE 60 > 15 → 0 分看空 | **0 分看空，但要说明为什么** — 不能只写 "PE 60 > 15" |

## Agent 评估架构

```
                    ┌─────────────────────────────┐
                    │  Task 3 · 主控 Claude        │
                    │  读取 raw_data + features     │
                    └────────┬────────────────────┘
                             │
        ┌──────────────┬─────┴───────┬──────────────────┐
        │              │             │                  │
┌───────▼───────┐ ┌────▼──────┐ ┌────▼───────────┐ ┌────▼──────────┐
│ Sub-Agent A+B │ │Sub-Agent C│ │ Sub-Agent D+E  │ │ Sub-Agent F   │
│ 经典价值+成长  │ │ 宏观对冲  │ │ 技术趋势+中国价投│ │ 游资          │
│ 15 人         │ │ 7 人      │ │ 11 人          │ │ 24 人         │
└───────────────┘ └───────────┘ └────────────────┘ └───────────────┘
                             │
                    ┌────────▼──────────┐
                    │ Sub-Agent G+H+I   │
                    │ 量化+科技+AI卡位   │
                    │ 9 人              │
                    └───────────────────┘
```

名册共 66 位、9 大流派，**内嵌在二进制里**（源文件 `crates/uzi-investors/src/data/investors.json`，
运行时不读它）：
A 经典价值 6 · B 成长 9 · C 宏观对冲 7 · D 技术趋势 4 · E 中国价投 7 ·
F 游资 24 · G 量化 4 · H 科技领袖 4 · I Serenity 1。

**每个 sub-agent 的输入 —— 全部来自缓存与 `skills/` 资产，不需要 Rust 源码**：

| 输入 | 运行时来源 |
|---|---|
| 原始数据 | `.cache/{ticker}/raw_data.json` |
| 22 维评分 | `.cache/{ticker}/dimensions.json` |
| 规则引擎骨架（pass/fail + score，仅参考，可覆盖） | `.cache/{ticker}/panel.json` → `investors[]` |
| 语言风格 | `skills/deep-analysis/personas/{id}.yaml`（51 份；仅 flagship 有） + 参考 `panel.json` 的 `comment` |
| 方法论 / 流派关注点 | `skills/investor-panel/references/group-*.md` |
| 真实语录与持仓风格 | `skills/investor-panel/references/quotes-knowledge-base.md` |
| 席位 / 射程（F 组） | `.cache/{ticker}/raw_data.json` → `16_lhb.data.matched_youzi` + `skills/lhb-analyzer/references/seat-encyclopedia.md` |

> `features` 与 `investor_knowledge` 是二进制内部派生量，**不落盘**（源码落点
> `uzi_features::stock_features` / `uzi_investors::knowledge`，仅供维护者对照）。
> 你要的等价信息直接从上表的 `raw_data.json` / `dimensions.json` 取，或用自己的公开知识补，
> 不要去找这两个不存在的缓存文件。

**每个 sub-agent 的输出**：
- 每个投资者一个 `{signal, score, headline, reasoning}`
- headline 不是模板，是 agent 自己写的判断
- reasoning 引用具体数据 + 该投资者的投资哲学

## 每组 Sub-Agent 的 Prompt 模板

### Group A+B · 经典价值 + 成长 (15 人)

```
你要扮演以下 15 位投资大佬，逐一对 {stock_name} ({ticker}) 给出判断。

规则引擎已经跑过了，结果如下（仅供参考，你可以覆盖）：
{rule_engine_results_json}

真实信息：
{investor_knowledge_json}

你的任务：
- 对每个人，先想"如果我是他，我会怎么看这只票？"
- 引用公司的具体数据（ROE/PE/行业/护城河/现金流）
- 如果他实际持有或公开看好过这只票/行业，必须提到
- 如果他的投资哲学和这只票完全不匹配，直接说"不在我的能力圈"
- 分数可以和规则引擎不同——你是在模拟这个人的判断，不是跑公式

巴菲特的投资哲学：长期持有好生意，看 ROE/护城河/管理层，讨厌短线，买他能理解的
格雷厄姆的投资哲学：PE < 15, PB < 1.5, 安全边际，纯数字派
费雪的投资哲学：成长股投资，15 points，研发能力，管理层质量
芒格的投资哲学：好生意 + 好价格 + 好管理，逆向思维
邓普顿的投资哲学：逆向全球投资，在最悲观的时候买
卡拉曼的投资哲学：绝对安全边际 30%+，宁可错过也不亏损
林奇的投资哲学：PEG < 1，在日常生活中发现 tenbagger
欧奈尔的投资哲学：CANSLIM 7 要素，动量 + 基本面
蒂尔的投资哲学：0 到 1，垄断，秘密
木头姐的投资哲学：颠覆式创新 5 平台，S 曲线拐点，5 年改变游戏规则
（其余成员见 `panel.json` 中 `group == "B"` 的条目：马克·安德森 / 比尔·格利 / 纳瓦尔 / 布拉德·格斯特纳 / 查马斯）

对每个人，输出格式：
{
  "investor_id": "buffett",
  "signal": "bullish" / "bearish" / "neutral" / "skip",
  "score": 0-100,
  "headline": "一句话总结（必须引用具体数字或事实）",
  "reasoning": "2-3 句话的推理过程",
  "override_rule_engine": true/false,
  "override_reason": "如果覆盖了规则引擎的结论，说明为什么"
}
```

### Group C · 宏观对冲 (7 人)

```
你要扮演 7 位宏观对冲大佬。
他们关心的不是单只股票的基本面，而是：
- 当前宏观周期（加息/降息/滞胀/复苏）
- 这只票在宏观格局里的位置
- 风险/收益的不对称性
- 市场情绪是否过度
{macro_data}
{sentiment_data}
```

### Group D+E · 技术趋势 + 中国价投 (11 人)

```
技术派关心：
- Stage (1/2/3/4)
- 均线排列（多头/空头）
- MACD / 成交量
- 距 60 日高点的百分比
{kline_data}

中国价投关心：
- ROE 持续性
- 护城河深度
- 管理层是否"本分"（段永平用语）
- 现金流质量
- 估值是否在历史低位
{financials_data}
```

### Group F · 游资 (24 人)

```
游资的核心逻辑完全不同：
- 市场：只做 A 股，不碰港股美股
- 周期：T+1 到 1 周
- 选股：板块龙头 / 涨停板打板 / 龙虎榜信号
- 标的：市值 20-500 亿为主（具体看个人风格）

⚠️ 在射程外的游资已由 `uzi_investors::seat_db` 标 `signal: skip` / `confidence: 0`，
不需要 sub-agent 再评估；只有射程内（或被龙虎榜强制激活）的席位才评分。
如果这只票不是 A 股 → 全部 skip "不适合"

龙虎榜数据：{lhb_data}
近期涨停：{kline_data}
板块热度：{sentiment_data}
```

### Group G · 量化 (4 人)

```
量化系统关心：
- 动量因子（近 20/60/120 日涨幅）
- 价值因子（PE/PB 分位）
- 质量因子（ROE 稳定性 + FCF）
- 波动率
- 成交量异常
{technical_features}
```

### Group H+I · 科技领袖 + AI 卡位 (5 人)

```
科技领袖关心：
- 技术周期位置（AI / 半导体 / 平台迁移）
- 产品与生态的护城河（黄仁勋 / 马斯克 / 奥特曼 / 塞勒 各自视角）
- 资本开支、算力与资产配置
- 平台型公司的 TAM 与颠覆风险

AI 卡位猎手 Serenity 关心：
- 算力 / 数据 / 芯片供应链上的稀缺环节
- AI 资本开支的传导链上谁是"卖铲人"
{industry_data}
{kline_data}
```

## Claude 在 Task 3 的完整流程

1. **跑 Stage 1**：`uzi <ticker> --stage1` —— 内部 `uzi_cli::stages` 产出规则引擎骨架分
2. **读结果**：读 `.cache/{ticker}/panel.json`（顶层字段见 [`../investor-panel/SKILL.md`](../../investor-panel/SKILL.md)）
3. **Spawn 4-5 个 sub-agent**（用 Agent tool，可并行）：
   - Agent A+B: 经典价值 + 成长（15 人）
   - Agent C: 宏观对冲（7 人）
   - Agent D+E: 技术 + 中国价投（11 人）
   - Agent F: 游资（24 人）— 如果非 A 股直接全 skip
   - Agent G+H+I: 量化 + 科技领袖 + AI 卡位（9 人）
4. **每个 sub-agent 返回** 各自负责的投资者的 `{signal, score, headline, reasoning}`
5. **主 Claude 合并**：把 sub-agent 的结果覆盖到 `panel.json` 的对应投资者
6. **写入 `.cache/{ticker}/agent_analysis.json`**：定性结论进 `panel_insights`（≥30 字，含投票分布 + 多空分歧），
   覆盖后的 `panel.json` 由 `--stage2` 读取；校验在 `uzi_review::validator`

## 快速模式 vs 深度模式

- **快速模式**（`uzi <ticker>`）：数据 + 规则引擎骨架分，一把跑完（`lite` / `medium` 档适用）
- **深度模式**（`uzi <ticker> --stage1` + role-play + `uzi <ticker> --stage2`）：
  你 spawn 4–5 个并行 sub-agent 覆盖骨架分，`--depth deep` **必须**走这条路
- **单人模式（debug）**：直接读 `panel.json` 里某个 `investor_id` 的 Signal，
  或只对那一位做 role-play，不修改其余评委

## 关键约束

1. **Sub-agent 必须输出结构化 JSON**，不是自由文本
2. **Sub-agent 可以覆盖规则引擎的分数**，但必须给出 `override_reason`
3. **如果 sub-agent 和规则引擎分数差 > 30 分**，主 Claude 要在综合研判里标记为"分歧点"（`uzi_pipeline::synthesis`）
4. **游资组**：非 A 股直接全 skip；射程外席位由 `uzi_investors::seat_db` 预判为 `signal: skip` / `confidence: 0`，不需要 spawn agent
5. **每个 headline 必须引用具体数字或事实**——禁止"基本面良好"式的废话
