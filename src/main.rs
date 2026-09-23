//! codexSwitch — run a second Codex app instance on another model provider
//! and hand conversations over to it.
//!
//! Unofficial. Not affiliated with or endorsed by OpenAI.

mod appserver;
mod catalog;
mod config;
mod handoff;
mod projects;
mod rollout;
mod setup;
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

#[derive(Parser)]
#[command(
    name = "codexSwitch",
    version,
    about = "Codex アプリの2つ目のインスタンスを Z.ai / OpenRouter で起動し、会話を引き継ぐ（非公式ツール）",
    override_usage = "codexSwitch [-zai | -openrouter] [--model <MODEL>]\n       \
        codexSwitch init\n       \
        codexSwitch handoff <チャット名/ID> [-zai | -openrouter]\n       \
        codexSwitch status",
    after_help = "例:\n  \
        codexSwitch init                 最初の準備（設定とモデル一覧を作り、API キーの置き場所を案内する）\n  \
        codexSwitch                      前回と同じ内容で起動する\n  \
        codexSwitch -zai                 Z.ai で起動する\n  \
        codexSwitch -openrouter          OpenRouter で起動する\n  \
        codexSwitch handoff <チャット名/ID>  本体の会話を引き継ぐ\n\n\
        終了はアプリの画面から行います（起動中に -zai / -openrouter を切り替えるときも、先に終了します）。"
)]
struct Cli {
    /// 設定ファイル（既定: ~/.config/codex-switch/config.toml）
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    /// -zai / -openrouter の受け口。直接は使わない
    #[arg(long, hide = true)]
    provider: Option<String>,
    /// モデル（既定: 前回と同じ。-zai / -openrouter を付けたときはその既定モデル）
    #[arg(long)]
    model: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 最初の準備: 第2インスタンスの設定とモデル一覧を作り、API キーの置き場所を案内する
    Init,
    /// 本体の会話を第2インスタンスへ引き継ぐ（-zai / -openrouter で引き継ぎ先を選べる）
    #[command(override_usage = "codexSwitch handoff <チャット名/ID> [-zai | -openrouter]")]
    Handoff {
        /// 引き継ぐ会話の名前（一部でよい）または ID
        #[arg(value_name = "チャット名/ID")]
        query: String,
        /// -zai / -openrouter の受け口。直接は使わない
        #[arg(long, hide = true)]
        provider: Option<String>,
    },
    /// 第2インスタンスと設定の状態を表示する
    Status,
}

/// Options that take a value: the word after them is never a provider flag.
const VALUE_OPTIONS: [&str; 3] = ["--config", "--provider", "--model"];

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
    // clap's args_conflicts_with_subcommands would also reject the global
    // --config, so the launch-only options are checked here.
    if cli.command.is_some() && (cli.provider.is_some() || cli.model.is_some()) {
        bail!(
            "-zai / -openrouter はサブコマンドの後に書いてください（例: codexSwitch handoff <チャット名/ID> -zai）"
        );
    }
    let cfg = Config::load(cli.config.as_deref())?;
    match cli.command {
        None => launch(&cfg, cli.provider, cli.model),
        Some(Command::Init) => setup::run(&cfg),
        Some(Command::Handoff { query, provider }) => handoff::run(&cfg, &query, provider),
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
    if catalog::needs_fetch(p) {
        ui::step(&format!("{} からモデル一覧を取得しています", p.label));
    }
    match catalog::ensure(p)? {
        catalog::Outcome::Present => {}
        catalog::Outcome::Bundled => ui::step(&format!(
            "モデル一覧を作成しました: {}",
            ui::tilde(&p.catalog)
        )),
        catalog::Outcome::Fetched => ui::step(&format!(
            "モデル一覧を取得しました: {}",
            ui::tilde(&p.catalog)
        )),
    }

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
            "第2インスタンスが {} / {} で起動中です。-zai / -openrouter とモデルは起動時に読まれるため、\
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
    println!("起動先（既定のモデル）:");
    let mark = |ok: bool| if ok { "✓" } else { "✗" };
    for p in cfg.providers.values() {
        println!(
            "  {:<12} {:<40} キー {}  モデル一覧 {}",
            format!("-{}", p.name),
            p.model,
            mark(sidecar::has_key(p)),
            mark(p.catalog.is_file())
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
            expand(&["codexSwitch", "handoff", "abc", "-zai"]),
            ["codexSwitch", "handoff", "abc", "--provider", "zai"]
        );
    }

    #[test]
    fn other_arguments_are_left_alone() {
        for args in [
            &["codexSwitch"][..],
            &["codexSwitch", "-h"],
            &["codexSwitch", "-V"],
            &["codexSwitch", "--model", "-weird"],
            &["codexSwitch", "handoff", "--", "-title"],
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

    #[test]
    fn handoff_takes_only_the_chat_and_the_target() {
        for removed in ["--dry-run", "-y", "--yes", "--no-relaunch"] {
            assert!(Cli::try_parse_from(["codexSwitch", "handoff", "abc", removed]).is_err());
        }
        for removed in ["--model", "--message", "--name", "--timeout"] {
            assert!(Cli::try_parse_from(["codexSwitch", "handoff", "abc", removed, "x"]).is_err());
        }
    }

    #[test]
    fn config_goes_with_every_form() {
        let parse = |args: &[&str]| {
            Cli::try_parse_from(expand_provider_flags(args.iter().map(OsString::from))).unwrap()
        };
        let cli = parse(&[
            "codexSwitch",
            "--config",
            "c.toml",
            "handoff",
            "abc",
            "-zai",
        ]);
        assert!(cli.config.is_some() && matches!(cli.command, Some(Command::Handoff { .. })));
        let cli = parse(&["codexSwitch", "--config", "c.toml", "status"]);
        assert!(cli.config.is_some() && matches!(cli.command, Some(Command::Status)));
        let cli = parse(&["codexSwitch", "--config", "c.toml", "-zai"]);
        assert!(cli.config.is_some() && cli.provider.as_deref() == Some("zai"));
    }
}
