---
description: 只跑 66 人评审团，看 66 位投资大佬怎么投票
argument-hint: "[股票名称或代码]"
---

# 66 位评审团任务

用户输入: $ARGUMENTS

加载 `investor-panel` skill 并执行：

1. 先跑 Stage 1 拿基础数据与评委骨架分（上游的 `fetch_basic` / `fetch_financials`
   已内建，**由 `--stage1` 自动采集**）：

```bash
uzi $ARGUMENTS --stage1
```

2. 读评审团产物 `.cache/<ticker>/panel.json`：

```bash
jq '.investors | length' .cache/<ticker>/panel.json
jq '.vote_distribution' .cache/<ticker>/panel.json
jq '.panel_consensus' .cache/<ticker>/panel.json
jq '[.investors[].group] | group_by(.) | map({group: .[0], n: length})' .cache/<ticker>/panel.json
jq '[.investors[] | select(.signal=="bullish")] | sort_by(-.score) | .[0:5] | .[] | {name, score, comment}' .cache/<ticker>/panel.json
jq '[.investors[] | select(.signal=="bearish")] | sort_by(.score) | .[0:5] | .[] | {name, score, comment}' .cache/<ticker>/panel.json
```

3. 然后让 **66 位**投资者各自按自己的方法论 role-play 打分，按 **9 大流派**分组，
   参考 `investor-panel/references/group-{a..i}.md`：

| 组 | 流派 | 人数 |
|---|---|---|
| A | 经典价值 | 6 |
| B | 成长投资 | 9 |
| C | 宏观对冲 | 7 |
| D | 技术趋势 | 4 |
| E | 中国价投 | 7 |
| F | A股游资（含射程规则） | 24 |
| G | 量化系统 | 4 |
| H | 科技领袖 | 4 |
| I | AI 卡位猎手 Serenity | 1 |

4. 输出**评审团专题结论**，包含：
   - 66 人投票分布柱状图（`vote_distribution`）
   - 9 大流派态度对比
   - **The Great Divide 世纪分歧**：评分差最大的两位大佬对撞
   - 每人卡片可展开看打分逻辑 + 模拟评语 + 像素头像（跑 `uzi $ARGUMENTS --stage2`
     即可在完整报告里渲染评审团模块）

只读评审团产物，不写 `agent_analysis.json`、不做 22 维全量 agent 研判，速度更快。
