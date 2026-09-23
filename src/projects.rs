//! Sidebar project assignment in the app's `.codex-global-state.json`.
//!
//! The app writes this file wholesale while running, so it must only be edited
//! while the sidecar is stopped.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

const STATE_FILE: &str = ".codex-global-state.json";

pub enum Assignment {
    Assigned {
        project: String,
    },
    /// No project's root contains the thread's working directory.
    NoMatch,
}

/// Assigns `thread_id` to the project whose root most closely contains `cwd`.
pub fn assign(sidecar_home: &Path, thread_id: &str, cwd: &Path) -> Result<Assignment> {
    let path = sidecar_home.join(STATE_FILE);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("アプリの状態ファイルを読めません: {}", path.display()))?;
    let mut state: Value = serde_json::from_str(&text)
        .with_context(|| format!("アプリの状態ファイルを解釈できません: {}", path.display()))?;

    let Some((project_id, project_name)) = match_project(&state, cwd) else {
        return Ok(Assignment::NoMatch);
    };

    let root = state
        .as_object_mut()
        .context("状態ファイルの形式が想定と異なります")?;
    let assignments = root
        .entry("thread-project-assignments")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("thread-project-assignments の形式が想定と異なります")?;
    assignments.insert(
        thread_id.to_owned(),
        json!({"projectKind": "local", "projectId": project_id}),
    );

    std::fs::copy(&path, path.with_extension("json.codex-switch.bak"))?;
    let tmp = path.with_extension("json.codex-switch.tmp");
    // The app stores this file as compact single-line JSON.
    std::fs::write(&tmp, serde_json::to_string(&state)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(Assignment::Assigned {
        project: project_name,
    })
}

/// `(id, name)` of the local project with the longest root that contains `cwd`.
fn match_project(state: &Value, cwd: &Path) -> Option<(String, String)> {
    let projects = state.get("local-projects")?.as_object()?;
    let mut best: Option<(usize, String, String)> = None;
    for (key, proj) in projects {
        let id = proj["id"].as_str().unwrap_or(key);
        let name = proj["name"].as_str().unwrap_or(id);
        let roots = proj["rootPaths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        for root in roots {
            if cwd.starts_with(root) {
                let depth = Path::new(root).components().count();
                if best.as_ref().is_none_or(|(d, _, _)| depth > *d) {
                    best = Some((depth, id.to_owned(), name.to_owned()));
                }
            }
        }
    }
    best.map(|(_, id, name)| (id, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({"local-projects": {
            "p1": {"id": "p1", "name": "shop", "rootPaths": ["/dev/shop"]},
            "p2": {"id": "p2", "name": "ledger", "rootPaths": ["/dev/shop/ledger"]},
            "p3": {"id": "p3", "name": "sho", "rootPaths": ["/dev/sho"]}
        }})
    }

    #[test]
    fn picks_the_deepest_containing_root() {
        let s = state();
        assert_eq!(
            match_project(&s, Path::new("/dev/shop")).unwrap().1,
            "shop"
        );
        assert_eq!(
            match_project(&s, Path::new("/dev/shop/ledger/src"))
                .unwrap()
                .1,
            "ledger"
        );
    }

    #[test]
    fn matches_whole_path_components_only() {
        // "/dev/sho" must not claim "/dev/shop".
        assert_eq!(
            match_project(&state(), Path::new("/dev/shop/x"))
                .unwrap()
                .1,
            "shop"
        );
        assert!(match_project(&state(), Path::new("/elsewhere")).is_none());
    }
}
