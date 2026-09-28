//! Rollout (conversation) files: fork ancestry, copying, and context size.

use crate::threads::{self, Thread};
use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// A fork only stores the turns after the fork point and replays the rest from
/// its parent's rollout. Returns the id of that parent, if any.
pub fn forked_from(path: &Path) -> Result<Option<String>> {
    let file = File::open(path)
        .with_context(|| format!("会話ファイルを開けません: {}", path.display()))?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first)?;
    let v: Value = serde_json::from_str(&first)
        .with_context(|| format!("会話ファイルの先頭行を読めません: {}", path.display()))?;
    if v["type"] != "session_meta" {
        return Ok(None);
    }
    Ok(v["payload"]["forked_from_id"].as_str().map(str::to_owned))
}

/// The thread's rollout followed by every ancestor it was forked from.
/// All of them must be present in the destination home for a fork to load.
pub fn chain(conn: &Connection, thread: &Thread) -> Result<Vec<PathBuf>> {
    let mut paths = vec![thread.rollout.clone()];
    let mut current = thread.rollout.clone();
    while let Some(parent_id) = forked_from(&current)? {
        if paths.len() > 64 {
            bail!("フォーク元のたどりが深すぎます（循環の可能性）");
        }
        let parent = threads::by_id(conn, &parent_id)?
            .with_context(|| format!("フォーク元の会話 {parent_id} が見つかりません"))?;
        paths.push(parent.rollout.clone());
        current = parent.rollout;
    }
    Ok(paths)
}

#[derive(Debug, Clone, Copy)]
pub struct Usage {
    pub input_tokens: i64,
}

/// Input size of the most recent request, i.e. roughly what the first message
/// after a handoff will send. Checks the thread first, then its ancestors.
pub fn last_usage(paths: &[PathBuf]) -> Option<Usage> {
    paths.iter().find_map(|p| last_usage_in(p).ok().flatten())
}

fn last_usage_in(path: &Path) -> Result<Option<Usage>> {
    let mut found = None;
    for line in BufReader::new(File::open(path)?).lines() {
        let line = line?;
        // Cheap pre-filter: most lines are large tool outputs or images.
        if !line.contains("\"token_count\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let info = &v["payload"]["info"];
        if let Some(input) = info["last_token_usage"]["input_tokens"].as_i64() {
            found = Some(Usage {
                input_tokens: input,
            });
        }
    }
    Ok(found)
}

pub enum Copied {
    Copied,
    AlreadyPresent,
}

/// Copies `path` (inside `src_home`) to the same relative location in
/// `dst_home`, verifying the result by reading it back. An identical existing
/// file is kept.
pub fn copy_into(src_home: &Path, dst_home: &Path, path: &Path) -> Result<Copied> {
    let rel = relative(path, src_home, cfg!(windows)).with_context(|| {
        format!(
            "会話ファイルが {} の外にあります: {}",
            src_home.display(),
            path.display()
        )
    })?;
    let dst = dst_home.join(rel);
    let data = snapshot(path)?;
    if dst.is_file() && fs::read(&dst)? == data {
        return Ok(Copied::AlreadyPresent);
    }
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = dst.with_extension("jsonl.codex-switch.tmp");
    fs::write(&tmp, &data).with_context(|| format!("コピーに失敗しました: {}", path.display()))?;
    if fs::read(&tmp)? != data {
        let _ = fs::remove_file(&tmp);
        bail!("コピーした内容が一致しません: {}", path.display());
    }
    fs::rename(&tmp, &dst)?;
    Ok(Copied::Copied)
}

/// The rollout up to its last complete line. The main app keeps an open
/// conversation's rollout open and appends to it, so a copy taken at any
/// moment must not end in half a line. (Rust opens files with full sharing
/// on Windows, so the app's handle does not get in the way.)
fn snapshot(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)
        .and_then(|mut f| f.read_to_end(&mut data))
        .with_context(|| format!("会話ファイルを読めません: {}", path.display()))?;
    if let Some(end) = data.iter().rposition(|&b| b == b'\n') {
        data.truncate(end + 1);
    }
    Ok(data)
}

/// `path` relative to `base`. Windows paths are case-insensitive, and the
/// thread database and the configuration may spell the home differently.
fn relative(path: &Path, base: &Path, windows: bool) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(base) {
        return Some(rel.to_path_buf());
    }
    if !windows {
        return None;
    }
    let (p, b) = (path.to_string_lossy(), base.to_string_lossy());
    let (p, b) = (
        crate::config::strip_verbatim(&p).into_owned(),
        crate::config::strip_verbatim(&b).into_owned(),
    );
    let b = b.trim_end_matches(['\\', '/']);
    let head = p.get(..b.len())?;
    let rest = p.get(b.len()..)?;
    (head.eq_ignore_ascii_case(b) && rest.starts_with(['\\', '/']))
        .then(|| PathBuf::from(&rest[1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_complete_lines_and_keeps_an_identical_copy() {
        let root =
            std::env::temp_dir().join(format!("codex-switch-rollout-{}", std::process::id()));
        let (src_home, dst_home) = (root.join("src"), root.join("dst"));
        let path = src_home.join("sessions").join("a.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let copy = || copy_into(&src_home, &dst_home, &path).unwrap();
        let copied = || fs::read(dst_home.join("sessions").join("a.jsonl")).unwrap();

        // The app is halfway through writing the second line.
        fs::write(&path, b"{\"a\":1}\n{\"b\":").unwrap();
        assert!(matches!(copy(), Copied::Copied));
        assert_eq!(copied(), b"{\"a\":1}\n");
        assert!(matches!(copy(), Copied::AlreadyPresent));

        // Once the line is finished, the copy follows.
        fs::write(&path, b"{\"a\":1}\n{\"b\":2}\n").unwrap();
        assert!(matches!(copy(), Copied::Copied));
        assert_eq!(copied(), b"{\"a\":1}\n{\"b\":2}\n");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn relative_paths() {
        let rel = |p: &str, b: &str, w| relative(Path::new(p), Path::new(b), w);
        assert_eq!(
            rel("/home/u/.codex/sessions/a.jsonl", "/home/u/.codex", false),
            Some(PathBuf::from("sessions/a.jsonl"))
        );
        assert_eq!(
            rel("/home/u/.codex2/a.jsonl", "/home/u/.codex", false),
            None
        );
        // Windows: case-insensitive, whole components only.
        assert_eq!(
            rel(
                "c:\\users\\you\\.codex\\sessions\\a.jsonl",
                "C:\\Users\\YOU\\.codex",
                true
            ),
            Some(PathBuf::from("sessions\\a.jsonl"))
        );
        assert_eq!(
            rel(
                "C:\\Users\\YOU\\.codex2\\a.jsonl",
                "C:\\Users\\YOU\\.codex",
                true
            ),
            None
        );
        // Codex may record paths with the extended-length prefix.
        assert_eq!(
            rel(
                "\\\\?\\C:\\Users\\YOU\\.codex\\a.jsonl",
                "C:\\Users\\YOU\\.codex",
                true
            ),
            Some(PathBuf::from("a.jsonl"))
        );
    }
}
