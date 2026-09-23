//! codexSwitch — run a second Codex app instance on another model provider
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

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use config::Config;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "codexSwitch",
    version,
    about = "Codex アプリの2つ目のインスタンスを別プロバイダで起動し、会話を引き継ぐ（非公式ツール）",
    override_usage = "codexSwitch [-<プロバイダ>] [--model <MODEL>]\n       codexSwitch <COMMAND>",
    after_help = "例:\n  \
        codexSwitch                 前回と同じプロバイダ・モデルで起動する\n  \
        codexSwitch -zai            Z.ai で起動する\n  \
        codexSwitch -openrouter     OpenRouter で起動する\n  \
        codexSwitch handoff <会話>  本体の会話を引き継ぐ\n\n\
        終了はアプリの画面から行います（起動中にプロバイダを変えるときも、先に終了します）。",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// 設定ファイル（既定: ~/.config/codex-switch/config.toml）
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    /// 起動するプロバイダ。-zai のように「-名前」でも指定できる（既定: 前回と同じ）
    #[arg(long, value_name = "NAME")]
    provider: Option<String>,
    /// モデル（既定: 前回と同じ。プロバイダを指定したときはその既定モデル）
    #[arg(long)]
    model: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 本体の会話を第2インスタンスへ引き継ぐ
    Handoff {
        /// 会話の ID、またはタイトルの一部
        query: String,
        /// 引き継ぎ先のプロバイダ。-zai のようにも書ける（既定: 第2インスタンスの現在の設定）
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

/// Options that take a value: the word after them is never a provider flag.
const VALUE_OPTIONS: [&str; 6] = [
    "--config",
    "--provider",
    "--model",
    "--message",
    "--name",
    "--timeout",
];

/// Rewrites `-zai` into `--provider zai`. clap has no single-dash long
/// options: to it, `-zai` would be the short flags `-z -a -i`.
fn expand_provider_flags(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut after_value_option = false;
    let mut after_terminator = false;
    for (i, arg) in args.into_iter().enumerate() {
        let text = arg.to_str().map(str::to_owned);
        let name = match &text {
            Some(t) if i > 0 && !after_value_option && !after_terminator => provider_flag(t),
            _ => None,
        };
        after_value_option = text.as_deref().is_some_and(|t| VALUE_OPTIONS.contains(&t));
        after_terminator |= text.as_deref() == Some("--");
        match name {
            Some(name) => {
                out.push("--provider".into());
                out.push(name.into());
            }
            None => out.push(arg),
        }
    }
    out
}

/// `-zai` → `zai`. Single-letter flags such as `-y` and `-h` are left alone.
fn provider_flag(arg: &str) -> Option<&str> {
    let name = arg.strip_prefix('-')?;
    let valid = name.len() >= 2
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then_some(name)
}

fn main() -> ExitCode {
    // Rust ignores SIGPIPE, which makes `println!` panic when the output is
    // piped into something like `head`. Behave like other CLI tools instead.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse_from(expand_provider_flags(std::env::args_os()));
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
        None => launch(&cfg, cli.provider, cli.model),
        Some(Command::Handoff {
            query,
            provider,
            model,
            message,
            name,
            yes,
            no_relaunch,
            dry_run,
            timeout,
        }) => handoff::run(
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
        Some(Command::Status) => status(&cfg),
    }
}

/// Starts the sidecar. A running sidecar is never stopped here: quitting is
/// left to the app's own UI, so nothing running in it is cut off.
fn launch(cfg: &Config, provider: Option<String>, model: Option<String>) -> Result<()> {
    let active = sidecar::active(cfg)?;
    let p = cfg.provider(provider.as_deref().unwrap_or(&active.provider))?;
    // Without a provider, start exactly what ran last time.
    let model = match model {
        Some(m) => m,
        None if provider.is_none() && !active.model.is_empty() => active.model.clone(),
        None => p.model.clone(),
    };
    sidecar::ensure_key(p)?;

    if !sidecar::running(cfg).is_empty() {
        if p.name == active.provider && model == active.model {
            // Starting the app again makes the running one show its window.
            sidecar::show(cfg)?;
            ui::step(&format!(
                "第2インスタンスは {} / {model} で起動中です。ウィンドウを表示しました",
                p.name
            ));
            return Ok(());
        }
        bail!(
            "第2インスタンスが {} / {} で起動中です。プロバイダとモデルは起動時に読まれるため、\
             アプリを終了してから、もう一度実行してください\n  終了のしかた: {}",
            active.provider,
            active.model,
            sidecar::QUIT_HOWTO
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(args: &[&str]) -> Vec<String> {
        expand_provider_flags(args.iter().map(OsString::from))
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn provider_flags_become_provider_options() {
        assert_eq!(
            expand(&["codexSwitch", "-zai"]),
            ["codexSwitch", "--provider", "zai"]
        );
        assert_eq!(
            expand(&["codexSwitch", "-openrouter", "--model", "x/y:free"]),
            [
                "codexSwitch",
                "--provider",
                "openrouter",
                "--model",
                "x/y:free"
            ]
        );
        assert_eq!(
            expand(&["codexSwitch", "handoff", "abc", "-zai", "-y"]),
            ["codexSwitch", "handoff", "abc", "--provider", "zai", "-y"]
        );
    }

    #[test]
    fn other_arguments_are_left_alone() {
        for args in [
            &["codexSwitch"][..],
            &["codexSwitch", "-h"],
            &["codexSwitch", "-V"],
            &["codexSwitch", "--model", "-weird"],
            &["codexSwitch", "handoff", "--message", "-note", "abc"],
            &["codexSwitch", "handoff", "--", "-title"],
            &["codexSwitch", "handoff", "abc", "--dry-run"],
        ] {
            assert_eq!(expand(args), args);
        }
    }

    #[test]
    fn parses_the_launch_forms() {
        let cli = Cli::try_parse_from(expand_provider_flags(
            ["codexSwitch", "-openrouter", "--model", "m"].map(OsString::from),
        ))
        .unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.provider.as_deref(), Some("openrouter"));
        assert_eq!(cli.model.as_deref(), Some("m"));

        let cli = Cli::try_parse_from(["codexSwitch"]).unwrap();
        assert!(cli.command.is_none() && cli.provider.is_none());

        let cli = Cli::try_parse_from(expand_provider_flags(
            ["codexSwitch", "handoff", "abc", "-zai"].map(OsString::from),
        ))
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Handoff { provider: Some(ref p), .. }) if p == "zai"
        ));
    }
}
