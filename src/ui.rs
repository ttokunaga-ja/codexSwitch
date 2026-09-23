//! Terminal output helpers.

use anyhow::{Result, bail};
use chrono::{Local, TimeZone};
use std::io::{IsTerminal, Write};
use std::path::Path;

pub fn step(msg: &str) {
    println!("▶ {msg}");
}

/// Asks a y/N question. Refuses to guess when there is no terminal to ask on.
pub fn confirm(prompt: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("確認できない環境のため中止しました。端末から実行してください");
    }
    print!("{prompt} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Shows paths under the home directory as `~/...`.
pub fn tilde(path: &Path) -> String {
    let home = crate::config::home();
    let shown = crate::config::strip_verbatim(&path.to_string_lossy()).into_owned();
    match Path::new(&shown).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        Err(_) => shown,
    }
}

/// A key file's path as the user should type it: `~/...` in a Unix shell,
/// the full path on Windows, where neither cmd nor every tool knows `~`.
pub fn key_path(path: &Path) -> String {
    if cfg!(windows) {
        crate::config::strip_verbatim(&path.to_string_lossy()).into_owned()
    } else {
        tilde(path)
    }
}

/// A command that puts an API key into `path`, for the user's own shell.
pub fn key_hint(path: &Path) -> String {
    if cfg!(windows) {
        format!(
            "Set-Content -NoNewline -Path '{}' -Value '<キー>'   （PowerShell）",
            key_path(path)
        )
    } else {
        format!("printf %s '<キー>' > {}", key_path(path))
    }
}

pub fn date(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "-".to_owned())
}

pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        out.insert(0, '-');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(thousands(165_328), "165,328");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(-1_234_567), "-1,234,567");
    }
}
