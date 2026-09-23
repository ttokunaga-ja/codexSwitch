//! codex-switch — run a second Codex app instance on another model provider
//! and hand conversations over to it.
//!
//! Unofficial. Not affiliated with or endorsed by OpenAI.

mod appserver;
mod config;
mod handoff;
mod projects;
mod rollout;
mod sidecar;
mod threads;
mod ui;
// The script builders are unit-tested everywhere but only run on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
mod windows;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "codex-switch",
    version,
    about = "Codex アプリの2つ目のインスタンスを別プロバイダで起動し、会話を引き継ぐ（非公式ツール）"
)]
struct Cli {
    /// 設定ファイル（既定: ~/.config/codex-switch/config.toml）
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 第2インスタンスを指定したプロバイダで起動する
    Launch {
        /// プロバイダ（既定: 現在の設定）
        provider: Option<String>,
        /// モデル（既定: プロバイダの既定モデル）
        #[arg(long)]
        model: Option<String>,
        /// 起動中なら確認せずに再起動する
        #[arg(short, long)]
        yes: bool,
    },
    /// 本体の会話を第2インスタンスへ引き継ぐ
    Handoff {
        /// 会話の ID、またはタイトルの一部
        query: String,
        /// 引き継ぎ先のプロバイダ（既定: 第2インスタンスの現在の設定）
        #[arg(long)]
        provider: Option<String>,
        /// 引き継ぎ先のモデル（既定: プロバイダの既定モデル）
        #[arg(long)]
        model: Option<String>,
        /// 最初のメッセージ（既定: 引き継ぎの要約。常に読み取り専用で実行）
        #[arg(long)]
        message: Option<String>,
        /// 新しい会話の名前（既定: 元の名前（モデル名））
        #[arg(long)]
        name: Option<String>,
        /// 確認を省略する
        #[arg(short, long)]
        yes: bool,
        /// 第2インスタンスを終了・再起動しない（プロジェクト割り当ても行わない）
        #[arg(long)]
        no_relaunch: bool,
        /// 実行内容を表示するだけで何もしない
        #[arg(long)]
        dry_run: bool,
        /// 最初のメッセージの完了を待つ秒数
        #[arg(long, default_value_t = 600, value_name = "SECONDS")]
        timeout: u64,
    },
    /// 第2インスタンスと設定の状態を表示する
    Status,
}

fn main() -> ExitCode {
    // Rust ignores SIGPIPE, which makes `println!` panic when the output is
    // piped into something like `head`. Behave like other CLI tools instead.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("エラー: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let cfg = Config::load(cli.config.as_deref())?;
    match cli.command {
        Command::Launch {
            provider,
            model,
            yes,
        } => launch(&cfg, provider, model, yes),
        Command::Handoff {
            query,
            provider,
            model,
            message,
            name,
            yes,
            no_relaunch,
            dry_run,
            timeout,
        } => handoff::run(
            &cfg,
            &handoff::Options {
                query,
                provider,
                model,
                message,
                name,
                yes,
                no_relaunch,
                dry_run,
                timeout: Duration::from_secs(timeout),
            },
        ),
        Command::Status => status(&cfg),
    }
}

fn launch(cfg: &Config, provider: Option<String>, model: Option<String>, yes: bool) -> Result<()> {
    let active = sidecar::active(cfg)?;
    let p = cfg.provider(provider.as_deref().unwrap_or(&active.provider))?;
    let model = model.unwrap_or_else(|| p.model.clone());
    sidecar::ensure_key(p)?;

    let running = sidecar::running(cfg);
    if !running.is_empty() {
        println!(
            "第2インスタンスは起動中です。プロバイダは起動時に読まれるため、再起動が必要です。"
        );
        if cfg!(windows) {
            println!("Windows 版のアプリは強制終了します。実行中の作業は中断されます。");
        }
        let question = format!("{}して再起動しますか？", sidecar::QUIT_VERB);
        if !yes && !ui::confirm(&question)? {
            println!("中止しました。");
            return Ok(());
        }
        ui::step(&format!(
            "第2インスタンスを{}しています",
            sidecar::QUIT_VERB
        ));
        sidecar::quit(cfg, &running)?;
    }
    sidecar::set_active(cfg, p, &model)?;
    sidecar::launch(cfg)?;
    ui::step(&format!(
        "第2インスタンスを {} / {model} で起動しました",
        p.name
    ));
    Ok(())
}

fn status(cfg: &Config) -> Result<()> {
    let running = sidecar::running(cfg);
    let state = if running.is_empty() {
        "停止中".to_owned()
    } else {
        let pids: Vec<String> = running.iter().map(|p| p.to_string()).collect();
        format!("起動中（PID {}）", pids.join(", "))
    };
    println!("第2インスタンス : {state}");
    match sidecar::active(cfg) {
        Ok(a) => println!("現在の設定      : {} / {}", a.provider, a.model),
        Err(e) => println!("現在の設定      : 読めません（{e:#}）"),
    }
    println!("プロバイダ:");
    for p in cfg.providers.values() {
        let key = std::fs::metadata(&p.key_file)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        println!(
            "  {:<11} {:<30} キー {}  カタログ {}",
            p.name,
            p.model,
            if key { "✓" } else { "✗" },
            if p.catalog.is_file() { "✓" } else { "✗" }
        );
    }
    #[cfg(windows)]
    match windows::package(&cfg.windows_package) {
        Ok(p) => println!("アプリ          : {} {}", cfg.windows_package, p.version),
        Err(e) => println!("アプリ          : 見つかりません（{e:#}）"),
    }
    match sidecar::codex_bin(cfg, false) {
        Ok(bin) => {
            let version = std::process::Command::new(&bin)
                .arg("--version")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            let state = match version {
                Some(v) => v,
                None if cfg!(windows) && cfg.codex_bin.is_none() => {
                    "未準備。初回の handoff でアプリから複製します".to_owned()
                }
                None => "実行できません".to_owned(),
            };
            println!("codex           : {} ({state})", ui::tilde(&bin));
        }
        Err(e) => println!("codex           : 決められません（{e:#}）"),
    }
    println!("本体のホーム    : {}", ui::tilde(&cfg.source_home));
    println!("第2のホーム     : {}", ui::tilde(&cfg.sidecar_home));
    Ok(())
}
