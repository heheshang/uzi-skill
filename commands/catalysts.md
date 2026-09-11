---
name: catalysts
description: 构建催化剂日历 — 已发生事件 + 未来 60 天关键节点 + 影响分级
---

# /catalysts <股票代码>

## 工作流

1. 从 dim 15（事件驱动）提取历史事件 + 预排标准日程。催化剂日历由 Task 1.5
   研究工作流随 `--stage1` 一起生成，落在
   `dimensions["21_research_workflow"].data.catalyst_calendar`：

   ```bash
   uzi <ticker> --stage1
   ```

2. 取日历（stdout 是纯 JSON，可直接 `jq`）：

   ```bash
   uzi <ticker> --method catalysts
   ```

   或离线读缓存：

   ```bash
   jq '.dimensions["21_research_workflow"].data.catalyst_calendar' .cache/<ticker>/raw_data.json
   ```

## 输出

全部节点在 `events[]`，按 `date` 排序，每条含 `date` / `event` / `category` / `impact`（high/medium/low）：

- **已发生事件**（`category: past`，按时间倒序，影响分级 high/medium/low）
- **未来 30 天**（`next_30d`）：
  - 季报披露窗口（`category: earnings`，高影响）
  - 股东大会 / 投资者关系活动（`category: corporate`，中影响）
- **未来 60 天**：
  - 行业展会 / 新品发布窗口（`category: industry`，中影响）
- **宏观节点**：
  - 美联储 FOMC 会议（`category: macro`，参考）
- 统计：`high_impact_count` / `past_event_count` / `forward_event_count`

## 展示样式

```
2026-04-30  [HIGH] 2026 Q1 季报披露（关注营收/净利超预期与否）
2026-05-15  [MED]  股东大会
2026-06-15  [MED]  行业展会
2026-05-28  [LOW]  美联储 FOMC (参考)
```

## 完成检查

- `events` 按 `date` 升序排列。
- `high_impact_count` 等于 `events` 中 `impact == "high"` 的条数。
- `next_30d` 是 `events` 中日期不晚于「今天 + 30 天」的子集。
