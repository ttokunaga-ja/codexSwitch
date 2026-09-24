//! Copies a conversation from the main Codex app into the sidecar instance and
//! continues it there on another provider.

use crate::appserver::AppServer;
use crate::config::{Config, Provider};
use crate::rollout::{self, Copied};
use crate::threads::{self, Thread};
use crate::ui;
use crate::{catalog, projects, sidecar};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

/// How long to wait for the user to quit the sidecar before giving up.
const QUIT_WAIT: Duration = Duration::from_secs(600);
/// How long the first message may take to complete.
const TURN_WAIT: Duration = Duration::from_secs(600);

struct Plan<'a> {
    thread: Thread,
    chain: Vec<PathBuf>,
    provider: &'a Provider,
    model: String,
    name: String,
    message: String,
}

/// `provider` comes from `-zai` / `-openrouter`. Without it, the conversation
/// goes to what the sidecar runs now, model included, like a plain launch.
pub fn run(cfg: &Config, query: &str, provider: Option<String>) -> Result<()> {
    let conn = threads::open(&cfg.source_home)?;
    let thread = resolve(&conn, query)?;
    let chain = rollout::chain(&conn, &thread)?;
    drop(conn);

    let active = sidecar::active(cfg)?;
    let target = cfg.provider(provider.as_deref().unwrap_or(&active.provider))?;
    let model = match &provider {
        None if !active.model.is_empty() => active.model.clone(),
        _ => target.model.clone(),
    };
    sidecar::ensure_key(target)?;
    if catalog::needs_fetch(target) {
        ui::step(&format!("{} からモデル一覧を取得しています", target.label));
    }
    catalog::ensure(target)?;

    let plan = Plan {
        name: format!("{}（{}）", thread.label(), short_model(&model)),
        message: default_message(target, &model),
        thread,
        chain,
        provider: target,
        model,
    };
    print_plan(cfg, &plan);
    if !ui::confirm("\n続行しますか？")? {
        println!("中止しました。");
        return Ok(());
    }

    // The project assignment is written while the app is closed, and a new
    // conversation shows up only after a restart: the sidecar has to quit.
    if !sidecar::running(cfg).is_empty() {
        println!(
            "\n第2インスタンスをアプリの画面から終了してください。終了を確認したら続けます（Ctrl+C で中止）。\n  終了のしかた: {}",
            sidecar::QUIT_HOWTO
        );
        ui::step("第2インスタンスの終了を待っています");
        sidecar::wait_for_quit(cfg, QUIT_WAIT)?;
    }

    let result = execute(cfg, &plan);

    // Relaunch even on failure so the sidecar is never left closed. On
    // failure, go back to what it was running before.
    let (p, m) = match (&result, cfg.provider(&active.provider)) {
        (Err(_), Ok(p)) => (p, active.model.as_str()),
        _ => (plan.provider, plan.model.as_str()),
    };
    ui::step(&format!("第2インスタンスを -{} で起動しています", p.name));
    let launched = sidecar::set_active(cfg, p, m).and_then(|()| sidecar::launch(cfg));
    if let Err(e) = launched {
        eprintln!("  起動に失敗しました: {e:#}");
        eprintln!("  手動で起動してください: codexSwitch -{}", p.name);
    }

    let new_id = result?;
    println!("\n完了しました。");
    println!("  会話: {}", plan.name);
    println!("  ID  : {new_id}");
    println!(
        "  CLI で続ける場合: CODEX_HOME={} codex resume {new_id}",
        ui::tilde(&cfg.sidecar_home)
    );
    Ok(())
}

fn resolve(conn: &rusqlite::Connection, query: &str) -> Result<Thread> {
    let mut found = threads::search(conn, query)?;
    match found.len() {
        0 => bail!("該当する会話が見つかりません: {query}"),
        1 => Ok(found.remove(0)),
        n => {
            println!("{n} 件の会話が該当しました。ID で指定してください。\n");
            for t in &found {
                println!(
                    "  {}  {}  {}\n      {} / {} / {}",
                    t.id,
                    ui::date(t.updated_ms),
                    t.label(),
                    t.model,
                    t.provider,
                    ui::tilde(&t.cwd)
                );
            }
            bail!("会話を1つに絞れませんでした");
        }
    }
}

fn print_plan(cfg: &Config, p: &Plan) {
    let size: u64 = p
        .chain
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();
    println!("引き継ぎ元   : {}", p.thread.label());
    println!(
        "               {} / {} / {}",
        p.thread.model,
        p.thread.provider,
        ui::tilde(&p.thread.cwd)
    );
    println!("               id {}", p.thread.id);
    println!(
        "引き継ぎ先   : 第2インスタンス（{}）/ {} / {}",
        ui::tilde(&cfg.sidecar_home),
        p.provider.name,
        p.model
    );
    println!(
        "コピー       : 会話ファイル {} 件（{:.1} MB）{}",
        p.chain.len(),
        size as f64 / 1e6,
        if p.chain.len() > 1 {
            "※フォーク元を含む"
        } else {
            ""
        }
    );
    println!("最初のメッセージ（読み取り専用で実行）:");
    for line in p.message.lines() {
        println!("    {line}");
    }
    match rollout::last_usage(&p.chain) {
        Some(u) => println!(
            "送信する量   : 約 {} トークン（元の会話の直近の入力量）",
            ui::thousands(u.input_tokens)
        ),
        None => println!("送信する量   : 不明"),
    }
}

fn execute(cfg: &Config, p: &Plan) -> Result<String> {
    ui::step("会話ファイルをコピーしています");
    for path in &p.chain {
        let status = match rollout::copy_into(&cfg.source_home, &cfg.sidecar_home, path)? {
            Copied::Copied => "コピー",
            Copied::AlreadyPresent => "既にあり",
        };
        println!("    {status}: {}", ui::tilde(path));
    }

    let codex = sidecar::codex_bin(cfg, false)?;
    if !codex.is_file() && codex.components().count() > 1 {
        ui::step("codex の実行ファイルを準備しています（初回のみ）");
    }
    let codex = sidecar::codex_bin(cfg, true)?;

    ui::step("フォークしています");
    let mut server = AppServer::spawn(&codex, &cfg.sidecar_home, Some(&p.thread.cwd))?;
    let forked = server.request(
        "thread/fork",
        json!({"threadId": p.thread.id, "modelProvider": p.provider.name, "model": p.model}),
        Duration::from_secs(300),
    )?;
    let new_id = forked["thread"]["id"]
        .as_str()
        .context("thread/fork の応答にスレッド ID がありません")?
        .to_owned();
    server.request(
        "thread/name/set",
        json!({"threadId": new_id, "name": p.name}),
        Duration::from_secs(30),
    )?;

    // A thread with no messages is hidden from the app's sidebar, so one turn
    // is required. It runs read-only because nobody is watching it.
    ui::step("最初のメッセージを送っています（読み取り専用）");
    server.request(
        "turn/start",
        json!({
            "threadId": new_id,
            "input": [{"type": "text", "text": p.message, "text_elements": []}],
            "sandboxPolicy": {"type": "readOnly"},
        }),
        Duration::from_secs(60),
    )?;
    let outcome = match server.wait_turn(&new_id, TURN_WAIT) {
        Ok(o) => o,
        Err(e) => bail!("{e:#}\n  作成済みの会話 ID: {new_id}"),
    };
    for e in &outcome.errors {
        println!("    （途中のエラー）{e}");
    }
    if outcome.status != "completed" {
        bail!(
            "最初のメッセージが完了しませんでした（{}）: {}\n  作成済みの会話 ID: {new_id}",
            outcome.status,
            outcome.error.as_deref().unwrap_or("理由不明")
        );
    }
    println!("\n  --- {} の返答 ---", p.model);
    for line in outcome.reply.lines() {
        println!("  {line}");
    }
    println!("  ---");

    drop(server);

    match projects::assign(&cfg.sidecar_home, &new_id, &p.thread.cwd)? {
        projects::Assignment::Assigned { project } => {
            ui::step(&format!("プロジェクト「{project}」に割り当てました"))
        }
        projects::Assignment::NoMatch => ui::step(&format!(
            "{} を含むプロジェクトが第2インスタンスにありません（割り当てなし）",
            ui::tilde(&p.thread.cwd)
        )),
    }
    Ok(new_id)
}

fn default_message(p: &Provider, model: &str) -> String {
    format!(
        "{}（{}）へ引き継ぎました。ここまでの状況と、次に着手すべきことを3行以内で整理してください。\
         ファイルの変更やコマンドの実行はしないでください。",
        p.label, model
    )
}

/// `nex-agi/nex-n2.5-pro:free` → `nex-n2.5-pro:free`
fn short_model(model: &str) -> &str {
    model.rsplit('/').next().unwrap_or(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_vendor_prefixed_models() {
        assert_eq!(
            short_model("nex-agi/nex-n2.5-pro:free"),
            "nex-n2.5-pro:free"
        );
        assert_eq!(short_model("glm-5.3-flash"), "glm-5.3-flash");
    }
}
