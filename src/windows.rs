//! Windows support.
//!
//! The Codex app ships as an MSIX package, which has two consequences:
//! - executables inside the package cannot be run from outside it, so the
//!   bundled `codex.exe` is copied to a cache before it is used;
//! - the app has no execution alias, so the second instance is started with
//!   `Invoke-CommandInDesktopPackage`: it runs `cmd.exe` inside the package,
//!   and `cmd` sets the environment variables and starts the app.
//!
//! The script builders are plain functions so they are tested on every
//! platform; only running them is Windows-specific.

use std::path::Path;

/// `-EncodedCommand` payload: Base64 of the UTF-16LE script. This sidesteps
/// every quoting layer between this process and PowerShell.
pub fn encode_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// PowerShell single-quoted string literal.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Prints the newest installed version of package `name` as compact JSON.
pub fn package_script(name: &str) -> String {
    format!(
        "$p = Get-AppxPackage -Name {} | Sort-Object Version -Descending | Select-Object -First 1\n\
         if (-not $p) {{ exit 3 }}\n\
         [pscustomobject]@{{ InstallLocation = $p.InstallLocation; \
         PackageFamilyName = $p.PackageFamilyName; Version = $p.Version.ToString() }} \
         | ConvertTo-Json -Compress",
        ps_quote(name)
    )
}

/// Starts the app as a second instance with its own CODEX_HOME and user data.
///
/// `cmd.exe` runs inside the package, so it is allowed to start the packaged
/// executable, and the variables it sets are inherited by the app.
pub fn launch_script(
    family: &str,
    app_id: &str,
    install_location: &Path,
    home: &Path,
    user_data: &Path,
) -> String {
    let exe = format!("{}\\app\\ChatGPT.exe", install_location.display());
    let cmdline = format!(
        "/c set \"CODEX_HOME={home}\" && set \"CODEX_ELECTRON_USER_DATA_PATH={data}\" \
         && start \"\" \"{exe}\" \"--user-data-dir={data}\"",
        home = home.display(),
        data = user_data.display(),
    );
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         Invoke-CommandInDesktopPackage -PackageFamilyName {} -AppId {} -Command 'cmd.exe' -Args {}",
        ps_quote(family),
        ps_quote(app_id),
        ps_quote(&cmdline)
    )
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::{encode_command, launch_script, package_script};
    use anyhow::{Context, Result, bail};
    use serde_json::Value;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub struct Package {
        pub install_location: PathBuf,
        pub family_name: String,
        pub version: String,
    }

    fn powershell(script: &str) -> Result<String> {
        let script = format!("[Console]::OutputEncoding = [Text.Encoding]::UTF8\n{script}");
        let out = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
            ])
            .arg("-EncodedCommand")
            .arg(encode_command(&script))
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("powershell.exe を実行できません")?;
        if !out.status.success() {
            bail!(
                "PowerShell が失敗しました（{}）: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn package(name: &str) -> Result<Package> {
        let out = powershell(&package_script(name))
            .with_context(|| format!("Codex アプリ（パッケージ {name}）が見つかりません"))?;
        let v: Value = serde_json::from_str(out.trim())
            .with_context(|| format!("パッケージ情報を解釈できません: {}", out.trim()))?;
        let field = |k: &str| {
            v[k].as_str()
                .map(str::to_owned)
                .with_context(|| format!("パッケージ情報に {k} がありません"))
        };
        Ok(Package {
            install_location: PathBuf::from(field("InstallLocation")?),
            family_name: field("PackageFamilyName")?,
            version: field("Version")?,
        })
    }

    /// Path of the cached copy of the package's `codex.exe`. With `prepare`,
    /// the copy is made when missing and copies of older versions are removed.
    pub fn codex_copy(pkg: &Package, cache_dir: &Path, prepare: bool) -> Result<PathBuf> {
        let target = cache_dir.join(format!("codex-{}.exe", pkg.version));
        if target.is_file() || !prepare {
            return Ok(target);
        }
        std::fs::create_dir_all(cache_dir)?;
        let src = pkg
            .install_location
            .join("app")
            .join("resources")
            .join("codex.exe");
        let tmp = target.with_extension("exe.tmp");
        std::fs::copy(&src, &tmp)
            .with_context(|| format!("codex.exe をコピーできません: {}", src.display()))?;
        std::fs::rename(&tmp, &target)?;
        for entry in std::fs::read_dir(cache_dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("codex-") && name.ends_with(".exe") && entry.path() != target {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(target)
    }

    pub fn launch(package_name: &str, app_id: &str, home: &Path, user_data: &Path) -> Result<()> {
        let pkg = package(package_name)?;
        powershell(&launch_script(
            &pkg.family_name,
            app_id,
            &pkg.install_location,
            home,
            user_data,
        ))
        .context("第2インスタンスを起動できませんでした")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn base64_matches_known_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn encodes_utf16le_for_powershell() {
        // "a" is 61 00 in UTF-16LE.
        assert_eq!(encode_command("a"), "YQA=");
        // "日" is U+65E5, i.e. E5 65 in UTF-16LE.
        assert_eq!(encode_command("日"), "5WU=");
    }

    #[test]
    fn quotes_powershell_literals() {
        assert_eq!(ps_quote("C:\\Users\\O'Neil"), "'C:\\Users\\O''Neil'");
    }

    #[test]
    fn launch_script_sets_variables_inside_the_package() {
        let s = launch_script(
            "OpenAI.Codex_2p2nqsd0c76g0",
            "App",
            &PathBuf::from("C:\\Program Files\\WindowsApps\\OpenAI.Codex_1.0_x64__x"),
            &PathBuf::from("C:\\Users\\a b\\.codex-switch"),
            &PathBuf::from("C:\\Users\\a b\\AppData\\Local\\codex-switch\\user-data"),
        );
        assert!(s.contains("-PackageFamilyName 'OpenAI.Codex_2p2nqsd0c76g0' -AppId 'App'"));
        assert!(s.contains("set \"CODEX_HOME=C:\\Users\\a b\\.codex-switch\""));
        assert!(s.contains(
            "set \"CODEX_ELECTRON_USER_DATA_PATH=C:\\Users\\a b\\AppData\\Local\\codex-switch\\user-data\""
        ));
        assert!(s.contains(
            "start \"\" \"C:\\Program Files\\WindowsApps\\OpenAI.Codex_1.0_x64__x\\app\\ChatGPT.exe\""
        ));
        assert!(s.contains(
            "\"--user-data-dir=C:\\Users\\a b\\AppData\\Local\\codex-switch\\user-data\""
        ));
    }
}
