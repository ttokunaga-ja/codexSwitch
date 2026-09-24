//! Model catalogs: the file behind the app's model picker, one per provider.
//!
//! A missing catalog is created on demand: from the copy bundled with this
//! tool, or from the provider's own endpoint when it publishes one (Z.ai).

use crate::config::Provider;
use crate::ui;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

/// OpenRouter models checked to run Codex's tool calls: free ones, written in
/// the minimal form (OpenAI's own entries carry `tool_mode`, which breaks
/// other providers).
const OPENROUTER: &str = include_str!("../catalogs/openrouter.json");

pub fn bundled(provider: &str) -> Option<&'static str> {
    match provider {
        "openrouter" => Some(OPENROUTER),
        _ => None,
    }
}

/// Whether `ensure` would have to download the catalog, which takes a moment.
pub fn needs_fetch(p: &Provider) -> bool {
    !p.catalog.is_file() && bundled(&p.name).is_none() && p.catalog_url.is_some()
}

/// What `ensure` did.
pub enum Outcome {
    Present,
    Bundled,
    Fetched,
}

/// Makes sure the provider's catalog exists. Fetching needs the API key, so
/// call this after the key has been checked.
pub fn ensure(p: &Provider) -> Result<Outcome> {
    if p.catalog.is_file() {
        return Ok(Outcome::Present);
    }
    if let Some(dir) = p.catalog.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if let Some(text) = bundled(&p.name) {
        write(&p.catalog, text.as_bytes())?;
        return Ok(Outcome::Bundled);
    }
    let Some(url) = p.catalog_url else {
        bail!(
            "{} のモデル一覧がありません: {}",
            p.label,
            ui::tilde(&p.catalog)
        );
    };
    let key = std::fs::read_to_string(&p.key_file)?;
    let mut catalog = fetch(url, key.trim())?;
    widen_reasoning_levels(&mut catalog);
    write(
        &p.catalog,
        serde_json::to_string_pretty(&catalog)?.as_bytes(),
    )?;
    Ok(Outcome::Fetched)
}

/// GETs the catalog with curl, which ships with macOS and Windows 10+. The
/// key goes in through stdin, so it never shows up in a process listing.
fn fetch(url: &str, key: &str) -> Result<Value> {
    let mut child = Command::new("curl")
        .args(["-sS", "--fail", "--max-time", "60", "-H", "@-", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("curl を実行できません")?;
    child
        .stdin
        .take()
        .context("curl の標準入力を開けません")?
        .write_all(format!("authorization: Bearer {key}\n").as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!(
            "モデル一覧を取得できませんでした（{url}）: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let v: Value = serde_json::from_slice(&out.stdout).context("モデル一覧の形式が不正です")?;
    if !v["models"].is_array() {
        bail!("モデル一覧の形式が不正です（models がありません）");
    }
    Ok(v)
}

/// Z.ai's catalog lists fewer reasoning levels than its models accept, so the
/// app's Effort slider shows only two steps. `high` is the default because
/// the app cannot select `max`, and a default it cannot show would mislead.
pub fn widen_reasoning_levels(catalog: &mut Value) {
    let levels = json!([
        {"effort": "low", "description": "Light reasoning"},
        {"effort": "medium", "description": "Balanced reasoning"},
        {"effort": "high", "description": "Enhanced reasoning"},
        {"effort": "xhigh", "description": "Extended reasoning"},
        {"effort": "max", "description": "Deep reasoning"},
    ]);
    if let Some(models) = catalog["models"].as_array_mut() {
        for m in models {
            m["supported_reasoning_levels"] = levels.clone();
            m["default_reasoning_level"] = json!("high");
        }
    }
}

fn write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.codex-switch.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_minimal_and_valid() {
        let v: Value = serde_json::from_str(OPENROUTER).unwrap();
        let models = v["models"].as_array().unwrap();
        assert!(!models.is_empty());
        for m in models {
            assert!(m.get("tool_mode").is_none(), "{}", m["slug"]);
            for key in ["slug", "shell_type", "supported_reasoning_levels"] {
                assert!(m.get(key).is_some(), "{} lacks {key}", m["slug"]);
            }
        }
    }

    /// The Windows script cannot share code with this binary, so what both
    /// must agree on is checked here.
    #[test]
    fn the_windows_script_agrees_with_this_binary() {
        let script = include_str!("../windows/codexSwitch-script.ps1");
        let start = script.find("$openRouterCatalog = @'").unwrap();
        let body = &script[start..];
        let body = &body[body.find('\n').unwrap() + 1..];
        let embedded: Value = serde_json::from_str(&body[..body.find("\n'@").unwrap()]).unwrap();
        assert_eq!(embedded, serde_json::from_str::<Value>(OPENROUTER).unwrap());

        // Compared with runs of whitespace collapsed: the script aligns columns.
        let flat = script.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(script.contains(crate::config::MANAGED_BEGIN));
        // The first message of a handoff is the same in both.
        for sentence in [
            "へ引き継ぎました。ここまでの状況と、次に着手すべきことを3行以内で整理してください。",
            "ファイルの変更やコマンドの実行はしないでください。",
        ] {
            assert!(script.contains(sentence), "script lacks {sentence}");
            assert!(include_str!("handoff.rs").contains(sentence));
        }
        assert!(script.contains(crate::config::MANAGED_END));
        let cfg =
            crate::config::Config::load(Some(std::path::Path::new("/nonexistent/c.toml"))).unwrap();
        for p in cfg.providers.values() {
            let file =
                |path: &std::path::Path| path.file_name().unwrap().to_string_lossy().into_owned();
            for expected in [
                format!("{} = @{{ Label = '{}'", p.name, p.label),
                format!("Model = '{}'", p.model),
                format!("Catalog = '{}'", file(&p.catalog)),
                format!("Effort = '{}'", p.effort),
                format!("Key = '{}'", file(&p.key_file)),
                format!("BaseUrl = '{}'", p.base_url),
            ] {
                assert!(flat.contains(&expected), "script lacks {expected}");
            }
        }
    }

    #[test]
    fn widens_every_model() {
        let mut v = json!({"models": [
            {"slug": "a", "supported_reasoning_levels": [{"effort": "low"}], "default_reasoning_level": "max"},
            {"slug": "b", "supported_reasoning_levels": []},
        ]});
        widen_reasoning_levels(&mut v);
        for m in v["models"].as_array().unwrap() {
            assert_eq!(m["supported_reasoning_levels"].as_array().unwrap().len(), 5);
            assert_eq!(m["default_reasoning_level"], "high");
        }
    }
}
