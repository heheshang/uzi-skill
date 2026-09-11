# 机构级财务分析方法库

> 改编自 `anthropics/financial-services-plugins`，适配 A 股 / 港股 / 美股散户深度分析场景。

本目录记录 22 种机构级分析方法的**方法论**与**A 股落地参数**（18 种核心 + 5 个 Tier-1 续引入，见文末）。每种方法都有对应的 Rust 计算模块（`uzi-models` crate）：

| 方法 | Rust 模块 / 函数 | 源 SKILL |
|---|---|---|
| DCF 估值 | `uzi_models::fin_models :: compute_dcf` | financial-analysis/dcf-model |
| Comps 相对估值 | `uzi_models::fin_models :: build_comps_table` | financial-analysis/comps-analysis |
| 三表预测 | `uzi_models::fin_models :: project_three_stmt` | financial-analysis/3-statement-model |
| LBO 快速测试 | `uzi_models::fin_models :: quick_lbo` | financial-analysis/lbo-model |
| 并购增厚/摊薄 | `uzi_models::fin_models :: accretion_dilution` | investment-banking/merger-model |
| Porter 5 Forces + BCG | `uzi_models::deep_methods :: build_competitive_analysis` | financial-analysis/competitive-analysis |
| 首次覆盖报告 | `uzi_models::research_workflow :: build_initiating_coverage` | equity-research/initiating-coverage |
| 财报业绩解读 | `uzi_models::research_workflow :: build_earnings_analysis` | equity-research/earnings-analysis |
| 催化剂日历 | `uzi_models::research_workflow :: build_catalyst_calendar` | equity-research/catalyst-calendar |
| 投资逻辑追踪 | `uzi_models::research_workflow :: build_thesis_tracker` | equity-research/thesis-tracker |
| 晨报 | `uzi_models::research_workflow :: build_morning_note` | equity-research/morning-note |
| 量化选股筛选 | `uzi_models::research_workflow :: run_idea_screen` | equity-research/idea-generation |
| 行业综述 | `uzi_models::research_workflow :: build_sector_overview` | equity-research/sector-overview |
| 投委会备忘录 | `uzi_models::deep_methods :: build_ic_memo` | private-equity/ic-memo |
| 单位经济 | `uzi_models::deep_methods :: build_unit_economics` | private-equity/unit-economics |
| 价值创造计划 | `uzi_models::deep_methods :: build_value_creation_plan` | private-equity/value-creation-plan |
| 尽调清单 | `uzi_models::deep_methods :: build_dd_checklist` | private-equity/dd-checklist |
| 组合再平衡（大类配置） | `uzi_models::deep_methods :: build_portfolio_rebalance` | wealth-management/portfolio-rebalance |

## Tier-1 续引入（v3.8.0 · `uzi_models::tier1` 模块）

> 2026-06-04 第二批从 `anthropics/financial-services` 续引入的 5 个与个股研究强相关的方法，
> 各自有方法论文档（本目录）+ 纯函数模块（`uzi_models::tier1`）。
>
> **调用方式**：5 个方法**都已接到 CLI**，随单二进制 `uzi` 分发，**不需要 Rust 源码**。
> 它们不在 22 维管线里预计算，而是由 `--method` 现场读缓存算：单票走
> `uzi <ticker> --method <NAME>`，组合走 `uzi --portfolio <csv> --method <NAME>`
> （`--stage1` 仍要先跑过一次建缓存）。下表 `uzi_models::tier1::*` 是**移植出处**，
> 供源码维护者对照 —— 运行时不读源码。

| 方法 | CLI 入口 | Rust 模块 / 函数（移植出处） | 源 SKILL |
|---|---|---|---|
| AI 就绪度/卡位评估 | `uzi <ticker> --method ai-readiness` | `uzi_models::tier1::ai_readiness :: build_ai_readiness` | private-equity/ai-readiness（适配单票 + 复用 `ai_chokepoint_score`） |
| 财报前预览 | `uzi <ticker> --method earnings-preview` | `uzi_models::tier1::earnings_preview :: build_earnings_preview` | equity-research/earnings-preview |
| 模型增量更新 | `uzi <ticker> --method model-update` | `uzi_models::tier1::model_update :: build_model_update` | equity-research/model-update |
| 组合收益归因 | `uzi --portfolio <csv> --method returns` | `uzi_models::tier1::returns_attrib :: build_returns_attribution` | private-equity/returns-analysis（适配二级市场组合） |
| 组合再平衡（逐持仓+换手成本） | `uzi --portfolio <csv> --method rebalance` | `uzi_models::tier1::rebalance :: build_rebalance` | wealth-management/portfolio-rebalance（A 股适配：去 TLH + 印花税/佣金本地化） |

说明：Tier-1 的 `uzi_models::tier1::rebalance :: build_rebalance`（逐持仓 + A 股印花税/佣金换手成本）与既有 `uzi_models::deep_methods :: build_portfolio_rebalance`（资产大类配置漂移）分工互补，前者出**调仓交易清单**、后者看**大类配置偏离**。

## 设计原则（全部来自原 SKILL.md 的 CRITICAL CONSTRAINTS）

1. **公式 over 硬编码**：所有派生值都是"函数调用"，不是预计算的数字。改变假设，全链条联动更新。
2. **Step-by-step 可审计**：每个模块返回的 JSON 对象里都带 `methodology_log`，完整记录"每一步在算什么"。
3. **敏感性内置**：DCF 强制 5×5 敏感性表，中心格必须等于基础案例的每股内在价值（自检机制）。
4. **数据源优先级**：真数据 > 代理估算 > 默认值。所有默认值都显式标记为 DEFAULT_*。
5. **情景分析**：IC memo 强制 Bull / Base / Bear 三情景 + 概率 + 假设。

## A 股落地参数

| 参数 | 默认值 | 来源 |
|---|---|---|
| 无风险利率 (rf) | 2.5% | 10Y 中国国债 |
| 股权风险溢价 (ERP) | 6.0% | A 股历史 |
| 标准税率 | 25% | 企业所得税 |
| 高新税率 | 15% | 高新技术企业 |
| 终值永续增长 | 2.5% | 长期 GDP 名义 |
| Beta 默认 | 1.0 | 中性 |
| 债务比例 | 30% | A 股中位数 |
| 税前债务成本 | 4.5% | LPR + 0.5-1pp |

## 报告集成

- 新增 dim **`20_valuation_models`** — DCF / Comps / 3-stmt / LBO 打包
- 新增 dim **`21_research_workflow`** — Initiating / Earnings / Catalyst / Thesis / Morning / Screen / Sector
- 新增 dim **`22_deep_methods`** — IC Memo / Unit Economics / VCP / DD / Competitive / Rebalance

每个新 dim 都通过 `uzi_models::compute` 的 `compute_dim_20` / `compute_dim_21` / `compute_dim_22` 生成，不走 web 请求（纯计算），随 `uzi <ticker> --stage1` 在建模阶段（Task 1.5）一次跑出。

## 已有 / 新增对照

**之前我们有**：19 维数据采集 + 51 评委量化规则
**这次新增**：机构级财务建模层 (DCF/Comps/LBO/3-stmt) + 研究工作流产物 (首次覆盖/财报解读/催化剂/逻辑追踪) + 深度决策方法 (IC memo/DD/Porter/单位经济/VCP/再平衡)

改动后：**19 维 + 3 新 dim + 51 评委（规则引擎引用新特征） + 18 种分析方法**
