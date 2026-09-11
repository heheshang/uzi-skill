# Hooks · v3.9.4

## 文件清单

| 文件 | 用途 |
|---|---|
| `hooks.json` | Claude Code 的 SessionStart hook 配置（直接调 `session-start`，不走 polyglot 中转） |
| `session-start` | bash 脚本 · 在 SessionStart 时打印 skill 列表 + 工作流提醒，输出到 Claude `additionalContext` |
| `hooks-cursor.json` | Cursor IDE 用的 hook 配置（`sessionStart` → `./hooks/session-start`） |
| `README.md` | 本说明 |

> 本项目**不包含** `run-hook.cmd`。上游 v2.6 起已不再使用这个 polyglot bash/batch 中转脚本
> （`.cmd` 在 macOS Claude Code 安全策略下有权限/格式问题），`hooks.json` 直接调用 `session-start`。
> Windows 用户如确有需要，可自行补一个批处理包装。

## 安装

**无需源码构建**：仓库根目录自带预编译 `uzi`（macOS arm64），后台更新检查直接用它。

1. 确认二进制可用（缺失 / 非本机平台时 hook 会静默跳过，不报错）：

   ```bash
   ./uzi --version        # 仓库自带；或 cargo build --release -p uzi-cli 产出 target/release/uzi
   ```

2. 给 `session-start` 可执行权限：

   ```bash
   chmod +x hooks/session-start
   ```

如果只是手动 clone：

```bash
chmod +x hooks/session-start
```

## 后台更新检查

`session-start` 在 `UZI_NO_UPDATE_CHECK` 未设置时，后台触发一次更新检查：

```bash
uzi --update-prompt-file .cache/_global/update_prompt.md
```

- 二进制解析顺序：优先 `$PLUGIN_ROOT/target/release/uzi`（源码构建产物），其次 `command -v uzi`（含仓库根自带的预编译二进制已加入 `PATH` 的情形）；两者都找不到则**静默跳过**，hook 不报错。
- 有新版 → 写入提示文件；无新版 → **删除**该文件。**文件存在即表示要提示 agent**，`SKILL.md` 的 HARD-GATE-UPDATE-PROMPT 负责读取并展示。
- 用户回答后用 `uzi --update-answer <y|s|n> <版本>` 处理。

## v2.6 论坛 bug 修复说明

论坛反馈 "Claude plugin 执行不了"，原因是旧版 `hooks.json` 调用 `run-hook.cmd` 中转脚本，
而 `.cmd` 在 macOS Claude Code 安全策略下：

1. 权限检查未通过（Claude Code 可能拒绝执行 `.cmd` 后缀脚本）
2. polyglot bash/batch 用 `: <<'BATCH_SCRIPT'` heredoc 体操，对解释器有要求

v2.6 修复：`hooks.json` 改为**直接调** `session-start`（标准 sh 脚本，已有 shebang），
跳过 `run-hook.cmd` 中间层。本项目沿用这一方案，因此仓库里没有该中转脚本。

## 调试

若 SessionStart 没有触发：

1. 确认 `session-start` 有 `+x` 权限：`ls -l hooks/session-start`
2. 手动测试输出：`./hooks/session-start`（应看到一行 JSON 含 `additionalContext`）
3. 检查 Claude Code 日志（`Cmd+Shift+P → Developer: Open Logs`）
4. 若 Claude Code 报路径错误，确认 `${CLAUDE_PLUGIN_ROOT}` 正确解析
