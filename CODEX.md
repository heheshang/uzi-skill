# UZI-Skill · Codex 专属指引

> 本文件供 **OpenAI Codex CLI / codex-rescue agent** 读取.
> 作用：在短上下文场景下给 codex 一份浓缩的项目地图 · 避免走错目录 / 误报结构问题.
> 长版本见 [AGENTS.md](./AGENTS.md).

---

## 🚨 必读前 60 秒

1. **入口是 `uzi` 二进制 · 不是某个 `crates/*/src/main.rs` 里的 main**
   - ✅ 先构建，再直接运行：
     ```bash
     cargo build --release -p uzi-cli
     ./target/release/uzi <ticker>
     ```
   - ❌ 不要找 `run.py` 之类的 Python 入口（本项目没有）
   - ❌ `cargo run -p uzi-investors`（库 crate 没有 `main` · 别找）
   - ❌ 不要找 `skills/deep-analysis/scripts/`（没有 `scripts/` 目录）

2. **所有 Rust 业务代码在 `crates/`**（workspace 共 10 个成员）
   - `uzi-data` — 22 维 fetcher + 数据源 registry + CDP 浏览器兜底
   - `uzi-cli` — 参数解析 + `stages.rs` 的 `stage1` / `stage2` 编排
   - `uzi-pipeline` — 22 维评分 + 综合研判 `synthesis`
   - `uzi-models` — DCF / Comps / LBO / 3-Stmt / Merger
   - `uzi-investors` — 66 评委 db / criteria / persona YAML / 席位射程
   - `uzi-report` — HTML / SVG / panel 渲染
   - `uzi-review` — 自检 + `agent_analysis.json` 校验
   - `uzi-core` / `uzi-features` / `uzi-screen`

3. **零外部依赖**：没有 `pip install`，没有 `requirements.txt`，没有 akshare。
   浏览器兜底用**系统已装的 Chromium 系**（Chrome / Chromium / Edge / Brave），CDP 驱动，
   `uzi --browser-check` 可确认。

4. **测试从仓库根跑 workspace 级**（相对路径 / 内嵌 fixture 都假定根目录）：
   `cargo test --workspace`。上游 Python 的对拍 fixture 在 `tools/golden/`。

---

## 架构关键约定

### 单一 pipeline · 无 Legacy Fallback

```bash
uzi <ticker>                # 一把跑完 stage1 → stage2（快速模式）
uzi <ticker> --stage1       # 停在采集 + 建模 + 评分 + 评委骨架，等 agent
uzi <ticker> --stage2       # 合并 agent_analysis.json 出报告
```

上游 v3.0 的 `UZI_LEGACY=1` 老路径 fallback 在移植时**被删除**——Rust 版只有一条 pipeline。
**不要**去找 `rrt.collect_raw_data` / `UZI_LEGACY` 这类旧开关。

### Pipeline 数据流

```
crates/uzi-cli/src/stages.rs
  ├─ stage1(ticker)
  │   ├─ uzi-data::collect()            # 22 维 fetcher 并发采集 → .cache/<ticker>/raw_data.json
  │   ├─ uzi-models                      # Dims 20–22 机构建模（DCF/Comps/LBO/…）
  │   ├─ uzi-pipeline::score             # 22 维打分 → dimensions.json
  │   └─ uzi-pipeline::panel             # 66 评委规则引擎骨架 → panel.json
  │                                      # + _agent_review_context.json（analysis_input_hash 指纹）
  └─ stage2(ticker)
      ├─ uzi-review::validator           # 校验 agent_analysis.json → _agent_analysis_errors.json
      ├─ uzi-pipeline::synthesis         # 合并 agent 判断 → synthesis.json
      └─ uzi-report::assemble            # → reports/<ticker>_<date>/full-report.html
```

### 迁移历史快照

| 上游版本 | 上游物理迁移 | 本仓库落点 |
|---|---|---|
| v3.1.0 | rrt 1228 行纯函数 → 上游 score_fns 模块 | `crates/uzi-pipeline/src/score.rs` |
| v3.1.0 | stage1 preflight 166 行 → 上游 preflight_helpers 模块 | `crates/uzi-data/src/network_preflight.rs` |
| v3.2.0 | assemble_report 2377 行 → 5 个上游 report 子模块 | `crates/uzi-report/src`（7 个渲染模块） |

**重要**：上游的 grep 式测试（搜字符串断言 re-export）在移植后改为 Rust 单元测试 +
`tools/golden/` 逐字节对拍。改评分 / 渲染后必须跑 `cargo test --workspace`，否则 golden 会红。

---

## 审视任务清单模板（给你参考）

如果被要求审视本项目 · 按此流程查（`<repo-root>` 替换为你 clone 的目录）：

```bash
cd <repo-root>

# 1. 先构建，不要找 scripts/ 或 Python 入口
cargo build --release -p uzi-cli                 # ✅ 产物: target/release/uzi

# 2. 验证入口 + 离线报告模板
./target/release/uzi --help
./target/release/uzi --preview                   # 内置 mock fixture，不联网也能出报告

# 3. 全量测试（workspace，从仓库根跑）
cargo test --workspace
```

`--preview` 是**离线 mock 数据**路径（`crates/uzi-cli/src/preview.rs`）：用来验证报告模板，
不代表真实采集结果；真实产物以 `uzi <ticker> --stage1` 为准。

---

## 常见 codex 误判避坑

| 误判 | 真相 |
|---|---|
| "`run.py` / `scripts/` 目录缺失 · 结构有问题" | 本项目是 Rust 重实现，入口是 `uzi` 二进制；业务代码在 `crates/` |
| "`crates/uzi-data/src/fetch/*.rs` 没被 adapter 用 · 可删" | 它们仍是独立采集器（22 维 registry 的成员）· 不能删 |
| "某个 crate 没有 `main` · 有死代码" | 库 crate 本来就没有 `main`；唯一入口在 `uzi-cli` |
| "`uzi-report/src/renderer/` 的 stub 没被 assemble 用 · 可删" | 是后续迭代的占位 · 留给未来升级用 |
| "循环 import" | Rust 无循环依赖；workspace 依赖单向（见 `Cargo.toml`），不存在 Python 那种 `import` 环 |
| "`--method` 输出里混了日志 · 解析坏了" | `--method` 的 stdout 是**纯 JSON**，日志走 stderr —— 直接管道给 `jq`，不要 grep 文本 |
| "`tools/golden/*.py` 是死代码" | 它们是**有意**保留的上游 Python 差分 fixture，不是可删的残留 |
| "`agent_analysis.json` 字段没生效" | 先看 `_agent_analysis_errors.json`；error 会回退到脚本骨架，warning 才合并 |

---

## 文件大小红线

| 文件 | 当前 | 上限（触发 refactor 信号） |
|---|---|---|
| `crates/uzi-report/src/dim_viz.rs` | 1283 行 | > 1500（再拆） |
| `crates/uzi-report/src/assemble.rs` | 1144 行 | > 1500（再拆） |
| `crates/uzi-report/src/institutional.rs` | 1015 行 | > 1500（再拆） |
| `crates/uzi-pipeline/src/score.rs` | 595 行 | > 1000（再拆） |
| `crates/uzi-cli/src/stages.rs` | 648 行 | > 1000（再拆） |
| 任意 `crates/*/src/*.rs` | — | > 1500（审视是否拆模块） |

如果某文件突破红线 · 说明又开始堆屎山 · 该开新 refactor PR。

---

## 有疑问先做什么

1. 读 `AGENTS.md` 完整版
2. 读 `SKILL.md`（根索引 · 4 个 skill 怎么分派）
3. 读 `skills/deep-analysis/SKILL.md`（两段式工作流 + `agent_analysis.json` 契约）
4. 读 `tools/golden/README.md`（与上游 Python 的对拍机制）

**别瞎猜** · 所有答案都在上面 4 个文档里.
