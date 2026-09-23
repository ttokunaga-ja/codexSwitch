//! The second ("sidecar") Codex app instance: detect, quit, configure, launch.

use crate::config::{Config, MANAGED_BEGIN, MANAGED_END, Provider};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How the sidecar is stopped, for messages. The Windows app does not quit on
/// a close request (it keeps running in the background), so it is terminated.
#[cfg(not(windows))]
pub const QUIT_VERB: &str = "終了";
#[cfg(windows)]
pub const QUIT_VERB: &str = "強制終了";

/// How to quit the app from its own UI. Closing the window is not enough on
/// either OS: the app keeps running (on Windows, in the notification area).
#[cfg(target_os = "macos")]
pub const QUIT_HOWTO: &str =
    "第2インスタンスのウィンドウを前面にして ⌘Q（ウィンドウを閉じるだけでは終了しません）";
#[cfg(windows)]
pub const QUIT_HOWTO: &str = "タスクバーの通知領域にある第2インスタンスのアイコンを右クリックし、\
     一番下の「Exit」を選ぶ（×ボタンで閉じても、通知領域で動き続けます）";
#[cfg(not(any(target_os = "macos", windows)))]
pub const QUIT_HOWTO: &str = "アプリのメニューから終了する";

/// Main-process PIDs of the running sidecar.
pub fn running(cfg: &Config) -> Vec<Pid> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::OnlyIfNotSet),
    );
    sys.processes()
        .values()
        .filter(|p| {
            let args: Vec<String> = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let stem = p
                .exe()
                .and_then(Path::file_stem)
                .map(|s| s.to_string_lossy().into_owned())
                .or_else(|| {
                    let first = args.first()?;
                    Some(Path::new(first).file_stem()?.to_string_lossy().into_owned())
                });
            is_sidecar_main(stem.as_deref(), &args, cfg)
        })
        .map(|p| p.pid())
        .collect()
}

/// Electron helpers (renderer, GPU, network...) carry the same
/// `--user-data-dir`; on Windows they even share the main executable. Chromium
/// marks every helper with `--type=`, so only the process without it is the
/// app itself.
fn is_sidecar_main(exe_stem: Option<&str>, args: &[String], cfg: &Config) -> bool {
    let flag = format!("--user-data-dir={}", cfg.user_data_dir.display());
    exe_stem == Some(cfg.app_process_name.as_str())
        && !args.iter().any(|a| a.starts_with("--type="))
        // On Windows the switch is passed quoted; compare without quotes.
        && args.iter().any(|a| a.replace('"', "") == flag)
}

/// Stops the sidecar and waits until no process of it is left. A failed
/// request on one PID is not an error by itself: helpers often exit with the
/// main process before they are asked. What counts is that nothing remains.
pub fn quit(cfg: &Config, pids: &[Pid]) -> Result<()> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        true,
        ProcessRefreshKind::nothing(),
    );
    for pid in pids {
        if let Some(p) = sys.process(*pid) {
            request_quit(p);
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

/// macOS: a regular termination request; the app shuts down cleanly.
#[cfg(unix)]
fn request_quit(p: &sysinfo::Process) {
    let _ = p.kill_with(sysinfo::Signal::Term);
}

/// Windows: a close request only hides the window, so terminate the main
/// process. Its helper processes exit with it.
#[cfg(windows)]
fn request_quit(p: &sysinfo::Process) {
    let _ = p.kill();
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

/// Brings a running sidecar's window to the front. The app allows one
/// instance per user-data dir, so the new process exits at once and hands
/// over to the running one, which then opens its window.
pub fn show(cfg: &Config) -> Result<()> {
    start(cfg)
}

/// Starts the sidecar and waits until it is actually running: both launchers
/// return before the app is up.
pub fn launch(cfg: &Config) -> Result<()> {
    std::fs::create_dir_all(&cfg.user_data_dir)?;
    start(cfg)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if !running(cfg).is_empty() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!("第2インスタンスの起動を30秒以内に確認できませんでした")
}

#[cfg(target_os = "macos")]
fn start(cfg: &Config) -> Result<()> {
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

#[cfg(windows)]
fn start(cfg: &Config) -> Result<()> {
    crate::windows::launch(
        &cfg.windows_package,
        &cfg.windows_app_id,
        &cfg.sidecar_home,
        &cfg.user_data_dir,
    )
}

#[cfg(not(any(target_os = "macos", windows)))]
fn start(_cfg: &Config) -> Result<()> {
    bail!("この OS では第2インスタンスの起動に対応していません。--no-relaunch を使ってください")
}

/// The `codex` binary for the sidecar's app-server. An explicit `codex_bin`
/// setting always wins. With `prepare`, missing files are created (Windows
/// copies the packaged binary); without it, the would-be path is returned.
pub fn codex_bin(cfg: &Config, prepare: bool) -> Result<PathBuf> {
    match &cfg.codex_bin {
        Some(p) => Ok(p.clone()),
        None => platform_codex_bin(cfg, prepare),
    }
}

/// The binary bundled with the app, so the sidecar's state is written by the
/// same version the GUI reads it with.
#[cfg(target_os = "macos")]
fn platform_codex_bin(cfg: &Config, _prepare: bool) -> Result<PathBuf> {
    let bundled = cfg.app_path.join("Contents/Resources/codex");
    Ok(if bundled.is_file() {
        bundled
    } else {
        PathBuf::from("codex")
    })
}

/// A copy of the package's binary: it cannot be run from inside the package.
#[cfg(windows)]
fn platform_codex_bin(cfg: &Config, prepare: bool) -> Result<PathBuf> {
    let pkg = crate::windows::package(&cfg.windows_package)?;
    crate::windows::codex_copy(&pkg, &cfg.cache_dir, prepare)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_codex_bin(_cfg: &Config, _prepare: bool) -> Result<PathBuf> {
    Ok(PathBuf::from("codex"))
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

    fn cfg(user_data_dir: &str) -> Config {
        let mut c = Config::load(Some(Path::new("/nonexistent/codex-switch.toml"))).unwrap();
        c.user_data_dir = PathBuf::from(user_data_dir);
        c
    }

    #[test]
    fn only_the_main_process_counts() {
        let c = cfg("C:\\Users\\x\\AppData\\Local\\codex-switch\\user-data");
        let args = |extra: &[&str]| -> Vec<String> {
            let mut v = vec!["C:\\Program Files\\WindowsApps\\app\\ChatGPT.exe".to_owned()];
            v.extend(extra.iter().map(|s| s.to_string()));
            v
        };
        let flag = "\"--user-data-dir=C:\\Users\\x\\AppData\\Local\\codex-switch\\user-data\"";
        // The app itself, with the switch passed quoted.
        assert!(is_sidecar_main(Some("ChatGPT"), &args(&[flag]), &c));
        // Its renderer: same exe and user data, but a helper.
        assert!(!is_sidecar_main(
            Some("ChatGPT"),
            &args(&["--type=renderer", flag]),
            &c
        ));
        // The main instance, which has no --user-data-dir.
        assert!(!is_sidecar_main(Some("ChatGPT"), &args(&[]), &c));
        // Another program that happens to get the same switch.
        assert!(!is_sidecar_main(Some("Other"), &args(&[flag]), &c));
    }

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
