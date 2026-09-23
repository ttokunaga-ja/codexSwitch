//! The second ("sidecar") Codex app instance: detect, quit, configure, launch.

use crate::config::{Config, MANAGED_BEGIN, MANAGED_END, Provider};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System, UpdateKind};

/// Main-process PIDs of the running sidecar. Helper processes (GPU, network,
/// crashpad) carry the same `--user-data-dir` but a different executable.
pub fn running(cfg: &Config) -> Vec<Pid> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::OnlyIfNotSet),
    );
    let flag = format!("--user-data-dir={}", cfg.user_data_dir.display());
    sys.processes()
        .values()
        .filter(|p| {
            let stem = p
                .exe()
                .and_then(Path::file_stem)
                .or_else(|| p.cmd().first().and_then(|a| Path::new(a).file_stem()))
                .map(|s| s.to_string_lossy().into_owned());
            stem.as_deref() == Some(cfg.app_process_name.as_str())
                && p.cmd().iter().any(|a| a.to_string_lossy() == flag)
        })
        .map(|p| p.pid())
        .collect()
}

/// Asks the sidecar to terminate and waits for it. Never force-kills: if it
/// does not exit, the user is told to close it instead.
pub fn quit(cfg: &Config, pids: &[Pid]) -> Result<()> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        true,
        ProcessRefreshKind::nothing(),
    );
    for pid in pids {
        if let Some(p) = sys.process(*pid) {
            match p.kill_with(Signal::Term) {
                Some(true) => {}
                Some(false) => bail!("第2インスタンス（PID {pid}）に終了を要求できませんでした"),
                None => bail!(
                    "この OS では第2インスタンスの自動終了に対応していません。手動で閉じてください"
                ),
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if running(cfg).is_empty() {
            // Give the app a moment to release its files.
            std::thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    bail!("第2インスタンスが20秒以内に終了しませんでした。ウィンドウを閉じてから再実行してください")
}

pub fn ensure_key(p: &Provider) -> Result<()> {
    let ok = std::fs::metadata(&p.key_file)
        .map(|m| m.len() > 0)
        .unwrap_or(false);
    if !ok {
        bail!(
            "{} の API キーが空です: {}\n  設定例: printf %s '<key>' > {}",
            p.label,
            p.key_file.display(),
            p.key_file.display()
        );
    }
    Ok(())
}

pub struct Active {
    pub provider: String,
    pub model: String,
}

/// Provider and model currently written in the sidecar's managed block.
pub fn active(cfg: &Config) -> Result<Active> {
    let text = read_config(cfg)?;
    let (start, end) = managed_range(&text, cfg)?;
    let body = &text[start + MANAGED_BEGIN.len()..end];
    let table: toml::Table = toml::from_str(body).context("管理ブロックを解釈できません")?;
    let get = |k: &str| {
        table
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    Ok(Active {
        provider: get("model_provider"),
        model: get("model"),
    })
}

/// Rewrites the managed block. Everything else in config.toml is left as is.
pub fn set_active(cfg: &Config, p: &Provider, model: &str) -> Result<()> {
    let path = cfg.sidecar_config();
    let text = read_config(cfg)?;
    let (start, end) = managed_range(&text, cfg)?;
    let block = format!(
        "{MANAGED_BEGIN}\nmodel_provider = {}\nmodel = {}\nmodel_catalog_json = {}\nmodel_reasoning_effort = {}\n{MANAGED_END}",
        toml_str(&p.name),
        toml_str(model),
        toml_str(&p.catalog.to_string_lossy()),
        toml_str(&p.effort),
    );
    let updated = format!(
        "{}{}{}",
        &text[..start],
        block,
        &text[end + MANAGED_END.len()..]
    );
    let tmp = path.with_extension("toml.codex-switch.tmp");
    std::fs::write(&tmp, updated)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn launch(cfg: &Config) -> Result<()> {
    std::fs::create_dir_all(&cfg.user_data_dir)?;
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-n")
            .arg("--env")
            .arg(format!("CODEX_HOME={}", cfg.sidecar_home.display()))
            .arg("--env")
            .arg(format!(
                "CODEX_ELECTRON_USER_DATA_PATH={}",
                cfg.user_data_dir.display()
            ))
            .arg(&cfg.app_path)
            .arg("--args")
            .arg(format!("--user-data-dir={}", cfg.user_data_dir.display()))
            .status()
            .context("open コマンドを実行できません")?;
        if !status.success() {
            bail!("アプリを起動できませんでした（open: {status}）");
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        bail!(
            "この OS での第2インスタンスの起動は未検証のため未対応です。\
             --no-relaunch を付けて実行し、アプリは手動で起動してください"
        )
    }
}

fn read_config(cfg: &Config) -> Result<String> {
    let path = cfg.sidecar_config();
    std::fs::read_to_string(&path)
        .with_context(|| format!("第2インスタンスの設定を読めません: {}", path.display()))
}

fn managed_range(text: &str, cfg: &Config) -> Result<(usize, usize)> {
    let start = text.find(MANAGED_BEGIN);
    let end = text.find(MANAGED_END);
    match (start, end) {
        (Some(s), Some(e)) if s < e => Ok((s, e)),
        _ => bail!(
            "{} に管理ブロックがありません。次の2行で囲んだブロックを用意してください:\n  {MANAGED_BEGIN}\n  {MANAGED_END}",
            cfg.sidecar_config().display()
        ),
    }
}

/// A TOML string value, quoted and escaped as needed.
fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_strings_round_trip() {
        for s in [
            "glm-5.3-flash",
            "nex-agi/nex-n2.5-pro:free",
            "a\"b'c",
            "C:\\Users\\x\\a b.json",
        ] {
            let table: toml::Table = toml::from_str(&format!("v = {}", toml_str(s))).unwrap();
            assert_eq!(table["v"].as_str(), Some(s));
        }
    }
}
