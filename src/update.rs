//! `codexSwitch update`: replaces this binary with the latest GitHub release.
//!
//! Releases are built by CI from version tags (vX.Y.Z), so changes pushed to
//! main never reach users before they are tagged.

use crate::ui;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "ttokunaga-ja/codexSwitch";

/// The release asset for this platform.
const ASSET: &str = if cfg!(windows) {
    "codexSwitch-windows-x64.exe"
} else {
    "codexSwitch-macos"
};

const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// `codexSwitch version`: this version, and how it stands against the latest
/// release. `--version` stays offline: `update` and the installers read it.
pub fn version() -> Result<()> {
    let (tag, latest) = latest_release().map_err(|e| {
        e.context(format!(
            "codexSwitch {CURRENT}（最新の版を確認できませんでした）"
        ))
    })?;
    println!(
        "codexSwitch {CURRENT}（{}）",
        standing(CURRENT, &tag, &latest)
    );
    Ok(())
}

fn standing(current: &str, tag: &str, latest: &str) -> String {
    match parse(current).cmp(&parse(latest)) {
        Ordering::Less => {
            format!("新しい版 {tag} があります。codexSwitch update で更新できます")
        }
        Ordering::Equal => "最新です".to_owned(),
        Ordering::Greater => format!("最新のリリース {tag} より新しい版です"),
    }
}

/// The latest release's tag and version.
fn latest_release() -> Result<(String, String)> {
    let release: Value = serde_json::from_slice(&get(&format!(
        "https://api.github.com/repos/{REPO}/releases/latest"
    ))?)
    .context("リリースの情報を読めません")?;
    let tag = release["tag_name"].as_str().unwrap_or_default().to_owned();
    let version = tag
        .strip_prefix('v')
        .filter(|v| parse(v).is_some())
        .map(str::to_owned)
        .with_context(|| format!("最新のリリースの版を読み取れません: {tag:?}"))?;
    Ok((tag, version))
}

pub fn run() -> Result<()> {
    let current = CURRENT;
    ui::step("最新の版を確認しています");
    let (tag, latest) = latest_release()?;
    let latest = latest.as_str();
    if parse(latest) <= parse(current) {
        println!("最新です（v{current}）");
        return Ok(());
    }

    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .context("codexSwitch 自身の場所が分かりません")?;
    ui::step(&format!("v{current} → {tag} に更新します"));
    let base = format!("https://github.com/{REPO}/releases/download/{tag}");
    let sums = String::from_utf8(get(&format!("{base}/SHA256SUMS"))?)?;
    let expected =
        checksum(&sums, ASSET).with_context(|| format!("SHA256SUMS に {ASSET} がありません"))?;
    let bytes = get(&format!("{base}/{ASSET}"))?;
    if hex(&Sha256::digest(&bytes)) != expected {
        bail!("ダウンロードした {ASSET} のハッシュが一致しません");
    }

    let new = beside(&exe, "new");
    std::fs::write(&new, &bytes).with_context(|| format!("書き込めません: {}", new.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
    }
    // Before anything is replaced: the new binary must run here and be the
    // version the release claims, or every update would install it again.
    if let Err(e) = check_version(&new, latest) {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    replace(&exe, &new)?;
    ui::step(&format!("{tag} に更新しました: {}", ui::tilde(&exe)));
    Ok(())
}

/// Windows cannot delete a running exe, so `replace` leaves the old one
/// behind; the next run removes it.
#[cfg(windows)]
pub fn remove_leftover() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(beside(&exe, "old"));
    }
}

fn get(url: &str) -> Result<Vec<u8>> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "300", url])
        .output()
        .context("curl を実行できません")?;
    if !out.status.success() {
        bail!(
            "ダウンロードできませんでした（{url}）: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// The hash listed for `name` in `sha256sum` output.
fn checksum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `codexSwitch.<tag>` next to the running binary, with the platform's suffix.
fn beside(exe: &Path, tag: &str) -> PathBuf {
    exe.with_file_name(format!("codexSwitch.{tag}{}", std::env::consts::EXE_SUFFIX))
}

fn check_version(bin: &Path, version: &str) -> Result<()> {
    let out = Command::new(bin).arg("--version").output().with_context(|| {
        if cfg!(windows) {
            "新しい版を実行できません。スマート アプリ コントロールに止められた可能性があります（今の版はそのままです）"
        } else {
            "新しい版を実行できません（今の版はそのままです）"
        }
    })?;
    let shown = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || shown.trim() != format!("codexSwitch {version}") {
        bail!(
            "新しい版の確認に失敗しました（{}）。今の版はそのままです",
            shown.trim()
        );
    }
    Ok(())
}

/// A rename is atomic, and a running binary may be renamed on both systems;
/// Windows only refuses to overwrite it, so it is moved aside first.
fn replace(exe: &Path, new: &Path) -> Result<()> {
    let fail = |e: std::io::Error| {
        anyhow::Error::new(e).context(format!("置き換えられません: {}", exe.display()))
    };
    if cfg!(windows) {
        let old = beside(exe, "old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(fail)?;
        if let Err(e) = std::fs::rename(new, exe) {
            let _ = std::fs::rename(&old, exe);
            return Err(fail(e));
        }
        return Ok(());
    }
    std::fs::rename(new, exe).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(parse("0.10.0") > parse("0.9.1"));
        assert!(parse("1.0.0") > parse("0.99.99"));
        assert_eq!(parse("0.2.0"), Some((0, 2, 0)));
        for bad in ["0.2", "0.2.0.1", "0.2.x", "v0.2.0", ""] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn tells_where_this_version_stands() {
        assert_eq!(standing("0.2.2", "v0.2.2", "0.2.2"), "最新です");
        assert!(standing("0.2.2", "v0.10.0", "0.10.0").starts_with("新しい版 v0.10.0 があります"));
        assert!(standing("0.3.0", "v0.2.2", "0.2.2").contains("より新しい版です"));
    }

    #[test]
    fn reads_sha256sum_output() {
        let sums = "AB12  codexSwitch-macos\ncd34 *codexSwitch-windows-x64.exe\n";
        assert_eq!(checksum(sums, "codexSwitch-macos").as_deref(), Some("ab12"));
        assert_eq!(
            checksum(sums, "codexSwitch-windows-x64.exe").as_deref(),
            Some("cd34")
        );
        assert_eq!(checksum(sums, "codexSwitch"), None);
    }
}
