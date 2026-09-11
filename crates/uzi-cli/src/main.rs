//! `uzi` — Rust replacement for `run.py`, the UZI-Skill command-line entry point.
//!
//! Mirrors the upstream argument surface:
//!
//! ```text
//! uzi 600519.SH                          # full analysis
//! uzi 002217 --depth lite --no-browser   # 30s quick scan
//! uzi --versus 600519.SH 000858.SZ       # head-to-head
//! uzi --portfolio holdings.csv           # portfolio health
//! uzi 600519.SH --remote                 # public tunnel link
//! ```

use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "uzi",
    version = uzi_cli::VERSION,
    about = "游资（UZI）Skills · 个股深度分析",
    after_help = "示例: uzi 贵州茅台 --remote"
)]
struct Args {
    /// 股票代码或中文名 (如 600519.SH / AAPL / 贵州茅台)
    #[arg(default_value = "002273.SZ")]
    ticker: String,

    /// 公网链接模式 · 启动 Cloudflare Tunnel
    #[arg(long)]
    remote: bool,

    /// 远程模式缺少 cloudflared 时允许自动安装（默认只提示安装方式，不改系统）
    #[arg(long)]
    install_cloudflared: bool,

    /// 不自动打开浏览器
    #[arg(long)]
    no_browser: bool,

    /// 本地 HTTP 服务端口
    #[arg(long, default_value_t = 8976)]
    port: u16,

    /// 强制指定名称（跳过自动解析）
    #[arg(long, value_name = "CODE")]
    force_name: Option<String>,

    /// 不用缓存续跑，强制全量重抓
    #[arg(long)]
    no_resume: bool,

    /// 采集后建模崩溃时，从 raw_data.json 秒级续跑
    #[arg(long)]
    from_modeling: bool,

    /// 只跑 Stage 1（采集 + 建模 + 骨架分）后停下，等 agent 介入写 agent_analysis.json
    #[arg(long)]
    stage1: bool,

    /// 只跑 Stage 2（合并 agent_analysis.json 后合成 + 出报告），需先跑过 Stage 1
    #[arg(long)]
    stage2: bool,

    /// 打印单个分析方法的结果后退出（dcf / comps / lbo / ic-memo / ai-readiness …）
    #[arg(long, value_name = "NAME")]
    method: Option<String>,

    /// 分业务收入建模：discover（生成骨架）/ validate（对账校验）
    #[arg(long, value_name = "discover|validate")]
    segmental: Option<String>,

    /// 机械级自查：跑 self-review 并打印报告（exit 1=critical, 2=warning, 0=通过）
    #[arg(long)]
    stage_review: bool,

    /// 启用雪球登录态抓取
    #[arg(long)]
    enable_xueqiu_login: bool,

    /// 思考深度 · lite(1-2min) / medium(5-8min) / deep(15-20min)
    #[arg(long, value_parser = ["lite", "medium", "deep"])]
    depth: Option<String>,

    /// 锁定单一流派视角 · A价值/B成长/C宏观/D技术/E中国价投/F游资/G量化/H科技领袖派/I Serenity
    #[arg(long, value_parser = ["A", "B", "C", "D", "E", "F", "G", "H", "I"])]
    school: Option<String>,

    /// 多股横向对比（2-4 个代码 / 中文名）
    #[arg(long, num_args = 1.., value_name = "TICKER")]
    versus: Vec<String>,

    /// 组合批量分析（CSV 列含 ticker / weight / note）
    #[arg(long, value_name = "CSV")]
    portfolio: Option<String>,

    /// A+港股每日游资/Serenity 全市场筛选
    #[arg(long, value_parser = ["daily"])]
    screen: Option<String>,

    /// 每日筛选报告模式
    #[arg(long, default_value = "noon", value_parser = ["noon", "close"])]
    mode: String,

    /// 每日筛选市场，逗号分隔
    #[arg(long, default_value = "A,H")]
    markets: String,

    /// 每日筛选角色组，当前固定 F,I
    #[arg(long, default_value = "F,I")]
    schools: String,

    /// 每日候选上限，最多 10 只
    #[arg(long, default_value_t = 10)]
    top: usize,

    /// 最低实际成交额（本币）
    #[arg(long, default_value_t = 2e8)]
    min_turnover: f64,

    /// 每日筛选仅用行情横截面，不抓逐股增强
    #[arg(long)]
    snapshot_only: bool,

    /// 把产出（standalone html + 图 + 摘要）拷贝到该目录，并生成 index.html / report.meta.json
    #[arg(long, value_name = "DIR")]
    output_dir: Option<String>,

    /// 并行 fetcher 数
    #[arg(long, default_value_t = 6)]
    max_workers: usize,

    /// 只检查新版本后退出（跳过缓存与跳过标记，等价 `python -m lib.update_check --force`）
    #[arg(long)]
    check_update: bool,

    /// 跳过启动时的版本检查（等价 `UZI_NO_UPDATE_CHECK=1`）
    #[arg(long)]
    no_update_check: bool,

    /// 把新版提示写入该文件（无新版则删除该文件）；供 agent session hook 使用
    #[arg(long, value_name = "PATH")]
    update_prompt_file: Option<String>,

    /// 处理用户对版本提示的回答并持久化：`--update-answer <y|s|n> <version>`
    #[arg(long, num_args = 2, value_names = ["ANSWER", "VERSION"])]
    update_answer: Option<Vec<String>>,

    /// 用内置 mock 数据生成一份完整报告，离线预览模板（等价 `preview_with_mock.py`）
    #[arg(long)]
    preview: bool,

    /// 预热跨股公共缓存（A 股名称表 + 宏观/政策/护城河检索），等价 `prewarm_cache.py`
    #[arg(long)]
    prewarm: bool,

    /// 一次性交互式雪球登录（保存 cookie 供后续复用）
    #[arg(long)]
    xueqiu_login: bool,

    /// 打印雪球登录状态后退出
    #[arg(long)]
    xueqiu_status: bool,

    /// 探测可用的浏览器（CDP 兜底抓取依赖），打印路径后退出
    #[arg(long)]
    browser_check: bool,
}

/// The depth recorded in a cached snapshot's `_agent_review_context.json`.
///
/// `None` when there is no cache for this ticker, when the file is missing or
/// malformed, or when it carries no usable `depth` — callers then fall back to
/// the normal profile resolution.
fn recorded_depth(ticker: &str) -> Option<String> {
    let ti = uzi_cli::stages::resolve_cached_target(ticker, &[]).ok()?;
    let path = uzi_core::cache::cache_root()
        .join(&ti.full)
        .join("_agent_review_context.json");
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("depth")
        .and_then(|d| d.as_str())
        .filter(|d| !d.is_empty())
        .map(str::to_string)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Manual `--check-update` short-circuits everything else, like upstream's
    // `python -m lib.update_check --force`.
    if args.check_update {
        return match uzi_cli::update_check::check_for_update(true) {
            Some(info) => {
                println!("{}", uzi_cli::update_check::format_prompt(&info));
                Ok(())
            }
            None => {
                println!("✓ 已是最新版（或无需提示）");
                Ok(())
            }
        };
    }
    if args.no_update_check {
        std::env::set_var("UZI_NO_UPDATE_CHECK", "1");
    }

    // Agent session-hook contract: write or clear the prompt file, then exit.
    // Existence of the file is the signal, so a stale prompt cannot resurface.
    if let Some(path) = &args.update_prompt_file {
        let written = uzi_cli::update_check::write_update_prompt(std::path::Path::new(path));
        println!(
            "{}",
            if written.is_some() {
                format!("→ 已写入更新提示: {path}")
            } else {
                format!("✓ 无可用更新，已清除: {path}")
            }
        );
        return Ok(());
    }
    if let Some(pair) = &args.update_answer {
        let (answer, version) = (&pair[0], &pair[1]);
        println!("{}", uzi_cli::update_check::apply_answer(answer, version));
        return Ok(());
    }

    // `run.py`: `if args.enable_xueqiu_login: os.environ["UZI_XQ_LOGIN"] = "1"`.
    if args.enable_xueqiu_login {
        std::env::set_var("UZI_XQ_LOGIN", "1");
    }

    // ── XueQiu / browser maintenance commands ──
    if args.xueqiu_status {
        print!("{}", uzi_data::browser::xueqiu::status_text());
        return Ok(());
    }
    if args.xueqiu_login {
        use std::io::IsTerminal;
        std::env::set_var("UZI_XQ_LOGIN", "1");
        let interactive = std::io::stdin().is_terminal();
        if !uzi_data::browser::xueqiu::interactive_login(interactive).is_success() {
            std::process::exit(1);
        }
        return Ok(());
    }
    if args.browser_check {
        match uzi_data::browser::find_chromium() {
            Ok(path) => println!("✓ 浏览器: {}", path.display()),
            Err(e) => {
                println!("✗ 未找到浏览器: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // `preview_with_mock.py`: build the offline mock report and stop. Runs before
    // the banner because it needs no network or profile.
    if args.preview {
        uzi_cli::preview::main_preview()?;
        return Ok(());
    }

    // `prewarm_cache.py`: fill the shared `_global` cache and scan it before
    // distributing. Needs the network, so it runs before the banner and exits.
    if args.prewarm {
        let year = chrono::Datelike::year(&chrono::Utc::now());
        uzi_data::prewarm::main_prewarm(year, 4);
        return Ok(());
    }

    // `run.py::maybe_prompt_update()` runs before the banner; it is a no-op on a
    // non-TTY stdin and never blocks the flow.
    uzi_cli::update_check::maybe_prompt_update();

    // `uzi-screen`'s versus / portfolio / fund-holdings runners need the
    // single-stock pipeline, but the dependency direction is CLI -> screen, so the
    // pipeline is injected here at startup. Without it those runners report
    // `pipeline_unavailable`, matching upstream's failed-import behaviour.
    {
        let max_workers = args.max_workers;
        uzi_screen::providers::set_pipeline_runner(move |ticker| {
            let result = uzi_cli::stages::run(ticker, max_workers)?;
            Ok(result)
        });
    }

    // A resumed snapshot must be reviewed in the mode it was built: a `lite`
    // Stage 1 leaves 7 dims in the cache, and reviewing it under the ambient
    // `medium` default would flag the 13 unfetched dims as critical and block the
    // report. An explicit `--depth` still wins, and `--no-resume` builds a fresh
    // snapshot so the user's depth applies.
    let depth = match &args.depth {
        Some(d) => Some(d.clone()),
        None if !args.no_resume => recorded_depth(&args.force_name.clone().unwrap_or_else(|| args.ticker.clone())),
        None => None,
    };
    let profile = match uzi_cli::profile::get_profile(depth.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(2);
        }
    };
    uzi_cli::profile::apply_profile_to_env(&profile);
    if let Some(school) = &args.school {
        std::env::set_var("UZI_SCHOOL", school);
    }
    if args.no_resume {
        std::env::set_var("UZI_NO_RESUME", "1");
    }

    // ── Per-method entry points (the `commands/*.md` surface) ──
    // Runs *before* the banner so `--method` writes pure JSON to stdout: an agent
    // parsing this output must not have to strip a human-facing header.
    if let Some(name) = &args.method {
        let ticker = args.force_name.clone().unwrap_or_else(|| args.ticker.clone());
        let out = match &args.portfolio {
            Some(csv) => uzi_cli::methods::run_portfolio(csv, name)?,
            None => uzi_cli::methods::run_single(&ticker, name)?,
        };
        println!("{}", uzi_core::json::to_pretty(&out));
        return Ok(());
    }
    if let Some(action) = &args.segmental {
        let ticker = args.force_name.clone().unwrap_or_else(|| args.ticker.clone());
        let code = uzi_cli::methods::run_segmental(&ticker, action)?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
    if args.stage_review {
        let ticker = args.force_name.clone().unwrap_or_else(|| args.ticker.clone());
        let argv = vec!["uzi".to_string(), ticker];
        let code = uzi_review::stage_review::run(&argv);
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }

    println!("{}", "━".repeat(50));
    println!("🎯 游资（UZI）Skills v{} · 深度分析引擎", uzi_cli::VERSION);
    println!("{}", uzi_cli::profile::format_banner(&profile));
    let env = uzi_cli::serve::detect_environment();
    println!("   环境: 本地");
    println!(
        "   浏览器: {}",
        if env.has_browser { "✓" } else { "✗ (headless)" }
    );
    println!(
        "   Cloudflare: {}",
        if env.has_cloudflared {
            "✓"
        } else {
            "✗ 未安装"
        }
    );
    println!("{}", "━".repeat(50));

    // ── Daily screen mode ──
    if let Some(_mode) = &args.screen {
        let result = uzi_screen::run_daily_screen(
            &args.mode,
            &args.markets,
            &args.schools,
            args.top.min(10),
            args.min_turnover,
            args.snapshot_only,
        )?;
        if let Some(path) = result.get("report_path").and_then(|v| v.as_str()) {
            post_process(&args, &env, Path::new(path))?;
        }
        return Ok(());
    }

    // ── Head-to-head mode ──
    if !args.versus.is_empty() {
        if !(2..=4).contains(&args.versus.len()) {
            anyhow::bail!("--versus 需要 2-4 个代码，实际 {} 个", args.versus.len());
        }
        let path = uzi_screen::run_versus(&args.versus)?;
        post_process(&args, &env, Path::new(&path))?;
        return Ok(());
    }

    // ── Portfolio mode ──
    if let Some(csv) = &args.portfolio {
        let path = uzi_screen::run_portfolio(csv)?;
        post_process(&args, &env, Path::new(&path))?;
        return Ok(());
    }

    // ── Single-ticker analysis ──
    let ticker = args.force_name.clone().unwrap_or_else(|| args.ticker.clone());

    // ── Two-stage entry points (the agent workflow) ──
    // Stage 1 stops after scoring so the agent can review `panel.json` and write
    // `agent_analysis.json`; Stage 2 merges that override and assembles the
    // report. Running both without the agent step is `--stage1`-less `uzi <ticker>`.
    if args.stage2 {
        let path = uzi_cli::stages::stage2(&ticker)?;
        return post_process(&args, &env, Path::new(&path));
    }
    if args.stage1 {
        let result = uzi_cli::stages::stage1(&ticker, args.max_workers);
        print!("{}", uzi_cli::stages::stage1_handoff(&result, &ticker));
        return Ok(());
    }

    let result = if args.from_modeling {
        // Resume modeling+scoring from cached raw_data, then continue into the
        // report — a resume that stopped at the payload would leave the user with
        // no output for the work it just did.
        let resumed = uzi_cli::stages::stage1_modeling(&ticker)?;
        uzi_cli::stages::continue_to_report(&ticker, resumed)?
    } else {
        uzi_cli::stages::run(&ticker, args.max_workers)?
    };

    // Early exits (name not resolved / non-stock security) produce no report.
    if result.is_string() || result.get("status").is_some() {
        return Ok(());
    }

    let report = result
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            result
                .get("report_path")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("未生成报告"))?;
    post_process(&args, &env, Path::new(&report))
}

/// Shared output / browser / remote handling for every report-producing mode.
fn post_process(
    args: &Args,
    env: &uzi_cli::serve::Environment,
    standalone: &Path,
) -> anyhow::Result<()> {
    let standalone = if standalone.is_absolute() {
        standalone.to_path_buf()
    } else {
        std::env::current_dir()?.join(standalone)
    };
    let report_dir = uzi_cli::stages::report_dir_of(&standalone);

    println!("\n{}", "━".repeat(50));
    println!("📄 报告路径: {}", standalone.display());
    let size_kb = std::fs::metadata(&standalone).map(|m| m.len() / 1024).unwrap_or(0);
    println!("   大小: {} KB", size_kb);

    if let Some(out_dir) = &args.output_dir {
        if let Err(e) = export_output_dir(args, &report_dir, &standalone, out_dir, size_kb) {
            println!("⚠️  --output-dir 导出失败（不影响本地报告）: {}", e);
        }
    }

    if env.has_browser && !args.no_browser && !args.remote {
        open_browser(&standalone);
        println!("   🌐 已在浏览器中打开");
    }

    if args.remote {
        let server = uzi_cli::serve::ReportServer::start(&standalone, args.port)?;
        let filename = standalone
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        let (public_url, _tunnel) =
            uzi_cli::serve::start_cloudflare_tunnel(args.port, args.install_cloudflared);

        if let Some(url) = public_url {
            let full_url = format!("{}/{}", url, filename);
            println!("\n{}", "━".repeat(50));
            println!("📱 远程查看地址:");
            println!("   {}", full_url);
            println!("{}", "━".repeat(50));
            println!("\n发送这个链接到手机就能看报告；不要转发到公开渠道。");
            println!("按 Ctrl+C 停止服务。\n");
            if env.has_browser && !args.no_browser {
                open_browser_path(&full_url);
            }
        } else {
            println!("\n   本地访问: http://localhost:{}/{}", args.port, filename);
        }
        server.wait_until_interrupted();
    } else if !env.has_browser || args.no_browser {
        println!("\n💡 提示: 当前环境无法打开浏览器");
        println!("   方式 1: 下载文件到本地打开");
        println!(
            "   方式 2: uzi {} --remote  ← 生成公网链接，手机就能看",
            args.ticker
        );
    }

    println!("{}", "━".repeat(50));
    println!("✅ 完成!");
    Ok(())
}

/// `--output-dir`: copy the report directory and emit `index.html` + `report.meta.json`.
fn export_output_dir(
    args: &Args,
    report_dir: &Path,
    standalone: &Path,
    out_dir: &str,
    size_kb: u64,
) -> anyhow::Result<()> {
    let out = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out)?;

    for entry in std::fs::read_dir(report_dir)? {
        let entry = entry?;
        let target = out.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    std::fs::copy(standalone, out.join("index.html"))?;

    let one_liner = std::fs::read_to_string(report_dir.join("one-liner.txt"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let meta = serde_json::json!({
        "schema": 1,
        "ticker": args.ticker,
        "depth": args.depth.clone().unwrap_or_else(|| {
            std::env::var("UZI_DEPTH").unwrap_or_else(|_| "medium".into())
        }),
        "generated_at": format!("{}Z", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f")),
        "report_dir": report_dir.display().to_string(),
        "standalone": standalone.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
        "index": "index.html",
        "size_kb": size_kb,
        "one_liner": one_liner,
    });
    std::fs::write(
        out.join("report.meta.json"),
        uzi_core::json::to_pretty(&meta),
    )?;
    println!("   📦 已导出到: {}/index.html", out.display());
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn open_browser(path: &Path) {
    open_browser_path(&path.display().to_string());
}

fn open_browser_path(target: &str) {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(target).spawn();
}
