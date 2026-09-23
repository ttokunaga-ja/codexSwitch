//! Copies a conversation from the main Codex app into the sidecar instance and
//! continues it there on another provider.

use crate::appserver::AppServer;
use crate::config::{Config, Provider};
use crate::rollout::{self, Copied};
use crate::threads::{self, Thread};
use crate::ui;
use crate::{projects, sidecar};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

/// How long to wait for the user to quit the sidecar before giving up.
const QUIT_WAIT: Duration = Duration::from_secs(600);

pub struct Options {
    pub query: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub message: Option<String>,
    pub name: Option<String>,
    pub yes: bool,
    pub no_relaunch: bool,
    pub dry_run: bool,
    pub timeout: Duration,
}

struct Plan<'a> {
    thread: Thread,
    chain: Vec<PathBuf>,
    provider: &'a Provider,
    model: String,
    name: String,
    message: String,
}

pub fn run(cfg: &Config, o: &Options) -> Result<()> {
    let conn = threads::open(&cfg.source_home)?;
    let thread = resolve(&conn, &o.query)?;
    let chain = rollout::chain(&conn, &thread)?;
    drop(conn);

    let active = sidecar::active(cfg)?;
    let provider = cfg.provider(o.provider.as_deref().unwrap_or(&active.provider))?;
    let model = o.model.clone().unwrap_or_else(|| provider.model.clone());
    sidecar::ensure_key(provider)?;
    crate::catalog::ensure(provider)?;

    let plan = Plan {
        name: o
            .name
            .clone()
            .unwrap_or_else(|| format!("{}（{}）", thread.label(), short_model(&model))),
        message: o
            .message
            .clone()
            .unwrap_or_else(|| default_message(provider, &model)),
        thread,
        chain,
        provider,
        model,
    };
    let running = sidecar::running(cfg);
    let relaunch = !o.no_relaunch;

    print_plan(cfg, &plan, &running, relaunch);
    if o.dry_run {
        println!("\n--dry-run のため、ここで終了します。");
        return Ok(());
    }
    if !o.yes && !ui::confirm("\n続行しますか？")? {
        println!("中止しました。");
        return Ok(());
    }

    if relaunch && !running.is_empty() {
        println!(
            "\n第2インスタンスをアプリの画面から終了してください。終了を確認したら続けます（Ctrl+C で中止）。\n  終了のしかた: {}",
            sidecar::QUIT_HOWTO
        );
        ui::step("第2インスタンスの終了を待っています");
        sidecar::wait_for_quit(cfg, QUIT_WAIT)?;
    }

    let result = execute(cfg, &plan, relaunch, o.timeout);

    if relaunch {
        // Relaunch even on failure so the sidecar is never left closed. On
        // failure, go back to the provider it was running before.
        let (p, m) = match &result {
            Ok(_) => (plan.provider, plan.model.as_str()),
            Err(_) => match cfg.provider(&active.provider) {
                Ok(p) => (p, active.model.as_str()),
                Err(_) => (plan.provider, plan.model.as_str()),
            },
        };
        ui::step(&format!("第2インスタンスを {} で起動しています", p.name));
        let launched = sidecar::set_active(cfg, p, m).and_then(|()| sidecar::launch(cfg));
        if let Err(e) = launched {
            eprintln!("  起動に失敗しました: {e:#}");
            eprintln!("  手動で起動してください: codexSwitch -{}", p.name);
        }
    }

    let new_id = result?;
    println!("\n完了しました。");
    println!("  会話: {}", plan.name);
    println!("  ID  : {new_id}");
    if !relaunch {
        println!(
            "  第2インスタンスを終了して起動し直すと、一覧に表示されます: codexSwitch -{}",
            plan.provider.name
        );
    }
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

fn print_plan(cfg: &Config, p: &Plan, running: &[sysinfo::Pid], relaunch: bool) {
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
    println!("新しい名前   : {}", p.name);
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
    let state = match (running.is_empty(), relaunch) {
        (false, true) => format!(
            "起動中（PID {}）→ アプリの画面から終了してもらってから処理し、完了後に {} で起動します",
            running
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            p.provider.name
        ),
        (true, true) => format!("停止中 → 完了後に {} で起動します", p.provider.name),
        (false, false) => "起動中のまま処理します（プロジェクト割り当ては行いません）".to_owned(),
        (true, false) => "停止中のまま処理します（プロジェクト割り当ては行いません）".to_owned(),
    };
    println!("第2インスタンス: {state}");
}

fn execute(cfg: &Config, p: &Plan, assign_project: bool, timeout: Duration) -> Result<String> {
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
    let outcome = match server.wait_turn(&new_id, timeout) {
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

    let listed = server.request(
        "thread/list",
        json!({"limit": 100, "useStateDbOnly": true, "modelProviders": [p.provider.name]}),
        Duration::from_secs(60),
    )?;
    let visible = listed["data"]
        .as_array()
        .is_some_and(|a| a.iter().any(|t| t["id"] == new_id.as_str()));
    drop(server);
    if !visible {
        println!("    注意: 一覧にまだ表示されていません（ID: {new_id}）");
    }

    if assign_project {
        match projects::assign(&cfg.sidecar_home, &new_id, &p.thread.cwd)? {
            projects::Assignment::Assigned { project } => {
                ui::step(&format!("プロジェクト「{project}」に割り当てました"))
            }
            projects::Assignment::NoMatch => ui::step(&format!(
                "{} を含むプロジェクトが第2インスタンスにありません（割り当てなし）",
                ui::tilde(&p.thread.cwd)
            )),
        }
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
