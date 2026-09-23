//! Paths and provider definitions.
//!
//! Everything has a built-in default that matches the setup described in the
//! README. An optional TOML file (`~/.config/codex-switch/config.toml`, or the
//! path in `$CODEX_SWITCH_CONFIG`) overrides individual values.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Markers around the block this tool rewrites in the sidecar's config.toml.
/// Kept identical to the original `codex-or` shell launcher so both interoperate.
pub const MANAGED_BEGIN: &str = "# >>> codex-or managed: active provider >>>";
pub const MANAGED_END: &str = "# <<< codex-or managed: active provider <<<";

#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub label: String,
    pub model: String,
    pub catalog: PathBuf,
    pub effort: String,
    pub key_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// CODEX_HOME of the main Codex app (the handoff source).
    pub source_home: PathBuf,
    /// CODEX_HOME of the second (sidecar) instance.
    pub sidecar_home: PathBuf,
    /// Electron user-data dir of the sidecar. A distinct dir is what lets a
    /// second instance start next to the main one.
    pub user_data_dir: PathBuf,
    /// App bundle launched by `open`. Only the macOS launcher uses it so far.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub app_path: PathBuf,
    /// File stem of the app's main executable, used to find the running sidecar.
    /// `ChatGPT` on both macOS and Windows.
    pub app_process_name: String,
    /// `codex` binary for the sidecar's app-server. When unset, it is chosen per
    /// platform (see `sidecar::codex_bin`).
    pub codex_bin: Option<PathBuf>,
    /// Windows: the app's MSIX package name and its application id.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub windows_package: String,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub windows_app_id: String,
    /// Windows: where the copy of the package's `codex.exe` is kept.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub cache_dir: PathBuf,
    pub providers: BTreeMap<String, Provider>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    source_home: Option<String>,
    sidecar_home: Option<String>,
    user_data_dir: Option<String>,
    app_path: Option<String>,
    app_process_name: Option<String>,
    codex_bin: Option<String>,
    windows_package: Option<String>,
    windows_app_id: Option<String>,
    #[serde(default)]
    providers: BTreeMap<String, FileProvider>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileProvider {
    label: Option<String>,
    model: String,
    catalog: String,
    effort: String,
    key_file: String,
}

pub fn home() -> PathBuf {
    dirs::home_dir().expect("home directory is not available")
}

/// Expands a leading `~` or `~/`. Other paths are returned unchanged.
pub fn expand(p: &str) -> PathBuf {
    if p == "~" {
        return home();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home().join(rest);
    }
    PathBuf::from(p)
}

pub fn default_config_path() -> PathBuf {
    match std::env::var("CODEX_SWITCH_CONFIG") {
        Ok(p) if !p.is_empty() => expand(&p),
        _ => home().join(".config/codex-switch/config.toml"),
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_config_path);
        let file: FileConfig = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("設定ファイルを読めません: {}", path.display()))?;
            toml::from_str(&text)
                .with_context(|| format!("設定ファイルの形式が不正です: {}", path.display()))?
        } else {
            FileConfig::default()
        };

        let source_home = opt_path(file.source_home).unwrap_or_else(|| home().join(".codex"));
        let sidecar_home = opt_path(file.sidecar_home).unwrap_or_else(|| home().join(".codex-or"));
        let user_data_dir = opt_path(file.user_data_dir).unwrap_or_else(default_user_data_dir);
        let app_path = opt_path(file.app_path).unwrap_or_else(default_app_path);
        let app_process_name = file
            .app_process_name
            .unwrap_or_else(|| "ChatGPT".to_owned());
        let codex_bin = opt_path(file.codex_bin);
        let windows_package = file
            .windows_package
            .unwrap_or_else(|| "OpenAI.Codex".to_owned());
        let windows_app_id = file.windows_app_id.unwrap_or_else(|| "App".to_owned());

        let mut providers = default_providers(&sidecar_home);
        for (name, p) in file.providers {
            providers.insert(
                name.clone(),
                Provider {
                    label: p.label.unwrap_or_else(|| name.clone()),
                    name,
                    model: p.model,
                    catalog: expand(&p.catalog),
                    effort: p.effort,
                    key_file: expand(&p.key_file),
                },
            );
        }

        Ok(Self {
            source_home,
            sidecar_home,
            user_data_dir,
            app_path,
            app_process_name,
            codex_bin,
            windows_package,
            windows_app_id,
            cache_dir: local_data_dir().join("codex-switch"),
            providers,
        })
    }

    pub fn provider(&self, name: &str) -> Result<&Provider> {
        self.providers.get(name).with_context(|| {
            let known: Vec<&str> = self.providers.keys().map(String::as_str).collect();
            format!(
                "未知のプロバイダです: {name}（設定済み: {}）",
                known.join(", ")
            )
        })
    }

    pub fn sidecar_config(&self) -> PathBuf {
        self.sidecar_home.join("config.toml")
    }
}

fn opt_path(p: Option<String>) -> Option<PathBuf> {
    p.as_deref().map(expand)
}

fn default_providers(sidecar: &Path) -> BTreeMap<String, Provider> {
    let keys = home().join(".codex");
    let mut m = BTreeMap::new();
    m.insert(
        "zai".to_owned(),
        Provider {
            name: "zai".to_owned(),
            label: "Z.ai".to_owned(),
            model: "glm-5.3-flash".to_owned(),
            catalog: sidecar.join("zai_models.json"),
            effort: "high".to_owned(),
            key_file: keys.join("zai.key"),
        },
    );
    m.insert(
        "openrouter".to_owned(),
        Provider {
            name: "openrouter".to_owned(),
            label: "OpenRouter".to_owned(),
            model: "nex-agi/nex-n2.5-pro:free".to_owned(),
            catalog: sidecar.join("model_catalog.json"),
            effort: "low".to_owned(),
            key_file: keys.join("openrouter.key"),
        },
    );
    m
}

fn default_user_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Application Support/Codex OpenRouter/user-data")
    }
    // No spaces: the path travels through cmd.exe and a command-line switch.
    #[cfg(not(target_os = "macos"))]
    {
        local_data_dir().join("codex-switch").join("user-data")
    }
}

fn local_data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(home)
}

fn default_app_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Applications/ChatGPT.app")
    }
    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::new()
    }
}
