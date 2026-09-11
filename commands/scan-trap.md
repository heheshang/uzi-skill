---
description: 单独检查一只股票是不是杀猪盘 / 有没有被资金套路
argument-hint: "[股票名称或代码，可附加描述如「朋友推荐我买」]"
---

# 杀猪盘体检任务

用户输入: $ARGUMENTS

加载 `trap-detector` skill 并执行 8 信号扫描。
完整信号定义见 `skills/trap-detector/SKILL.md` 与
`skills/trap-detector/references/eight-signals.md`。

先跑 Stage 1（`18_trap` 维度由 `--stage1` 自动产出）：

```bash
uzi $ARGUMENTS --stage1
```

读扫描结果：

```bash
jq '.dimensions["18_trap"].data' .cache/<ticker>/raw_data.json
jq '.dimensions["18_trap"]' .cache/<ticker>/dimensions.json
```

## 8 个信号

1. 大量低质量账号同时推荐
2. 推荐话术模板化
3. 付费社群/直播间引流
4. 基本面与热度脱节
5. K线异常配合
6. "老师/股神"人设推广
7. 跨平台联动推广
8. 虚假研报/伪造消息

> 命中判定：同一信号命中 ≥2 个关键词才算该信号命中；"信号数"因此偏保守。

## 风险评级（按 `18_trap` 真实阈值）

| 命中信号数 | 评级 | `trap_score` | 建议 |
|---|---|---|---|
| 0–1 | 🟢 安全 | 9 | 数据正常，未发现明显推广痕迹 |
| 2–3 | 🟡 注意 | 7 | 发现 N 个推广信号，建议核实信息源 |
| 4–5 | 🟠 警惕 | 4 | 发现 N 个推广信号，强烈建议谨慎 |
| 6+ | 🔴 高度可疑 | 1 | 强烈建议回避，疑似杀猪盘特征 |

> `trap_score` 是**反向分**：越高越安全（1=最危险，9=最安全）。

输出**风险评级**（🟢 安全 / 🟡 注意 / 🟠 警惕 / 🔴 高度可疑）+ 每个信号的具体证据 + 给用户的明确建议。
非 🟢 时必须给出至少 1 条具体证据 URL。

如果用户提到"朋友推荐"、"群里说"、"老师带"等关键词，按
`skills/trap-detector/SKILL.md` 的用户关键词加权表（+1 / +2）**自动加重信号严重程度**，
并落到 `recommendation` 的措辞上。检索受限时必须说明"检索受限"，不能把"搜不到"等同于"安全"。
