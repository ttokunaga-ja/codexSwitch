//! `codexSwitch init`: prepares everything the sidecar needs, so that only the
//! API keys are left to the user. Safe to run again: what exists is kept, what
//! is missing is added, and config.toml is backed up before it is changed.

use crate::catalog::{self, Outcome};
use crate::config::{Config, Provider};
use crate::sidecar::{has_key, has_managed_block, managed_block, toml_str};
use crate::ui;
use anyhow::{Context, Result};
use std::io::{IsTerminal, Write};
use std::path::Path;

/// Top-level settings the managed block owns. Left outside it, they would
/// clash with it (TOML forbids a key twice).
const MANAGED_KEYS: [&str; 4] = [
    "model_provider",
    "model",
    "model_catalog_json",
    "model_reasoning_effort",
];

pub fn run(cfg: &Config) -> Result<()> {
    ui::step(&format!(
        "第2インスタンスの設定を用意しています（{}）",
        ui::tilde(&cfg.sidecar_home)
    ));
    std::fs::create_dir_all(&cfg.sidecar_home)?;
    config_file(cfg)?;

    if std::io::stdin().is_terminal() {
        println!();
        ui::step(
            "API キーを貼り付けて Enter を押してください（画面には表示されません）。使わないもの、あとでファイルに入れるものは、そのまま Enter",
        );
        for p in cfg.providers.values().filter(|p| !has_key(p)) {
            ask_key(p)?;
        }
    }

    println!();
    ui::step("モデル一覧を用意しています");
    for p in cfg.providers.values() {
        println!("    {:<16} : {}", p.label, prepare_catalog(p));
    }

    println!();
    ui::step("準備ができました");
    println!("\nAPI キーのファイル（使うものだけで構いません）:");
    for p in cfg.providers.values() {
        if has_key(p) {
            println!("  {:<16} {}  ✓", p.label, ui::key_path(&p.key_file));
        } else {
            println!("  {:<16} {}  未設定", p.label, ui::key_path(&p.key_file));
            println!("  {:<16} 入れ方: {}", "", ui::key_hint(&p.key_file));
        }
    }
    println!("\n起動:");
    for p in cfg.providers.values() {
        println!("  codexSwitch -{}", p.name);
    }
    Ok(())
}

fn config_file(cfg: &Config) -> Result<()> {
    let path = cfg.sidecar_config();
    let shown = ui::tilde(&path);
    if !path.exists() {
        std::fs::write(&path, new_config(cfg))?;
        println!("    作成しました: {shown}");
        return Ok(());
    }
    let original = std::fs::read_to_string(&path)?;
    let mut text = original.clone();
    let mut changes = Vec::new();
    if !has_managed_block(&text) {
        text = add_managed_block(&text, cfg)?;
        changes.push("管理ブロックを追加".to_owned());
    }
    let table: toml::Table = toml::from_str(&text)
        .with_context(|| format!("{shown} を解釈できません。形式を確認してください"))?;
    let defined = table.get("model_providers").and_then(|v| v.as_table());
    for p in cfg.providers.values() {
        let Some(base_url) = &p.base_url else {
            continue;
        };
        if defined.is_some_and(|t| t.contains_key(&p.name)) {
            continue;
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
        text.push_str(&provider_tables(p, base_url));
        changes.push(format!("[model_providers.{}] を追加", p.name));
    }
    if changes.is_empty() {
        println!("    変更なし: {shown}");
        return Ok(());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup = path.with_extension(format!("toml.bak.{stamp}"));
    std::fs::copy(&path, &backup)?;
    let tmp = path.with_extension("toml.codex-switch.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &path)?;
    println!(
        "    更新しました: {shown}（{}）\n    元のファイル: {}",
        changes.join("、"),
        ui::tilde(&backup)
    );
    Ok(())
}

fn default_provider(cfg: &Config) -> &Provider {
    cfg.providers
        .get("zai")
        .or_else(|| cfg.providers.values().next())
        .expect("at least one provider is defined")
}

fn new_config(cfg: &Config) -> String {
    let p = default_provider(cfg);
    let mut s = format!(
        "# Codex アプリの第2インスタンス（codexSwitch）の設定。\n\
         # 目印の2行で囲んだ管理ブロックは、codexSwitch が起動のたびに書き換えます。\n\n{}\n",
        managed_block(p, &p.model)
    );
    for p in cfg.providers.values() {
        if let Some(base_url) = &p.base_url {
            s.push('\n');
            s.push_str(&provider_tables(p, base_url));
        }
    }
    s
}

/// Adds the managed block to a config written by hand. The settings it takes
/// over are commented out where they were, and the provider they chose is
/// kept when this tool knows it.
fn add_managed_block(text: &str, cfg: &Config) -> Result<String> {
    let table: toml::Table = toml::from_str(text).context("config.toml を解釈できません")?;
    let current = |k: &str| table.get(k).and_then(|v| v.as_str());
    let (p, model) = match current("model_provider").and_then(|n| cfg.providers.get(n)) {
        Some(p) => (p, current("model").unwrap_or(&p.model).to_owned()),
        None => {
            let p = default_provider(cfg);
            (p, p.model.clone())
        }
    };
    let mut body = String::new();
    let mut top_level = true;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            top_level = false;
        }
        let key = trimmed.split('=').next().unwrap_or("").trim();
        if top_level && trimmed.contains('=') && MANAGED_KEYS.contains(&key) {
            body.push_str("# (codexSwitch init) ");
        }
        body.push_str(line);
    }
    Ok(format!("{}\n\n{body}", managed_block(p, &model)))
}

/// `[model_providers.<name>]` and its auth table. The key is read from its
/// file by a command, so no secret is written into config.toml.
fn provider_tables(p: &Provider, base_url: &str) -> String {
    let (command, args) = key_reader(&p.key_file);
    let args: Vec<String> = args.iter().map(|a| toml_str(a)).collect();
    format!(
        "[model_providers.{name}]\n\
         name = {label}\n\
         base_url = {url}\n\
         wire_api = \"responses\"\n\
         requires_openai_auth = false\n\n\
         [model_providers.{name}.auth]\n\
         command = {command}\n\
         args = [{args}]\n",
        name = p.name,
        label = toml_str(&p.label),
        url = toml_str(base_url),
        command = toml_str(&command),
        args = args.join(", "),
    )
}

#[cfg(not(windows))]
fn key_reader(key: &Path) -> (String, Vec<String>) {
    (
        "/bin/cat".to_owned(),
        vec![key.to_string_lossy().into_owned()],
    )
}

#[cfg(windows)]
fn key_reader(key: &Path) -> (String, Vec<String>) {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    (
        format!(r"{root}\System32\cmd.exe"),
        vec![
            "/c".to_owned(),
            "type".to_owned(),
            key.to_string_lossy().into_owned(),
        ],
    )
}

fn ask_key(p: &Provider) -> Result<()> {
    let key = rpassword::prompt_password(format!("  {}: ", p.label))?;
    let key = key.trim();
    if key.is_empty() {
        return Ok(());
    }
    save_key(&p.key_file, key)?;
    println!("    保存しました: {}", ui::key_path(&p.key_file));
    Ok(())
}

fn save_key(path: &Path, key: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(key.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// One line on the state of a provider's model catalog. A catalog that has to
/// be fetched waits for the key; a failed fetch is retried at launch.
fn prepare_catalog(p: &Provider) -> String {
    if catalog::needs_fetch(p) && !has_key(p) {
        return "キーを入れたあと、最初の起動で取得します".to_owned();
    }
    match catalog::ensure(p) {
        Ok(Outcome::Present) => format!("あり（{}）", ui::tilde(&p.catalog)),
        Ok(Outcome::Bundled) => format!("作成しました（{}）", ui::tilde(&p.catalog)),
        Ok(Outcome::Fetched) => format!("取得しました（{}）", ui::tilde(&p.catalog)),
        Err(e) => format!("取得できませんでした。最初の起動でもう一度取得します（{e:#}）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut c = Config::load(Some(Path::new("/nonexistent/codex-switch.toml"))).unwrap();
        c.sidecar_home = std::path::PathBuf::from("/home/u/.codex-switch");
        c
    }

    #[test]
    fn a_new_config_parses_and_defines_every_provider() {
        let c = cfg();
        let t: toml::Table = toml::from_str(&new_config(&c)).unwrap();
        assert_eq!(t["model_provider"].as_str(), Some("zai"));
        let providers = t["model_providers"].as_table().unwrap();
        for name in ["zai", "openrouter"] {
            let p = providers[name].as_table().unwrap();
            assert_eq!(p["wire_api"].as_str(), Some("responses"));
            assert_eq!(p["requires_openai_auth"].as_bool(), Some(false));
            assert!(p["auth"]["command"].as_str().is_some());
        }
    }

    #[test]
    fn a_hand_written_config_keeps_its_provider() {
        let c = cfg();
        let hand = "model_provider = \"openrouter\"\nmodel = \"x/y:free\"\nmodel_reasoning_effort = \"low\"\n\n[model_providers.openrouter]\nname = \"OpenRouter\"\n";
        let out = add_managed_block(hand, &c).unwrap();
        assert!(has_managed_block(&out));
        let t: toml::Table = toml::from_str(&out).unwrap();
        assert_eq!(t["model_provider"].as_str(), Some("openrouter"));
        assert_eq!(t["model"].as_str(), Some("x/y:free"));
        assert!(out.contains("# (codexSwitch init) model_provider = \"openrouter\""));
        // A key with the same prefix inside a table is not touched.
        assert!(out.contains("[model_providers.openrouter]\nname = \"OpenRouter\""));
    }
}
