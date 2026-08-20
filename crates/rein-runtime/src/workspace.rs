//! Workspace layout (§11) and the config/secret boundary (invariant 27).
//!
//! `.rein/`: workspace.yaml, providers.lock, policies/, plans/, skills/,
//! ledger.db, objects/, cache/, logs/, tmp/. `configRoot` (default
//! `~/.config/rein/`) is a *separate* tree: credentials must never resolve
//! from a directory written by model output or network sync — enforced in
//! [`SecretBroker::open`], not requested in a comment.

use rein_core::ids::{SecretRefId, WorkspaceRef};
use rein_core::secretref::Redactor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const WORKSPACE_SCHEMA: &str = "rein.workspace/v1";

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("no .rein workspace at or above {0}")]
    NotFound(PathBuf),
    #[error("{0} already contains a .rein workspace")]
    AlreadyExists(PathBuf),
    #[error("config root {config} is inside the workspace root {workspace} — credentials must never resolve from a model-writable tree (invariant 27)")]
    ConfigInsideWorkspace { config: PathBuf, workspace: PathBuf },
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("workspace.yaml: {0}")]
    Manifest(String),
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> WorkspaceError + '_ {
    move |source| WorkspaceError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub schema: String,
    pub workspace_ref: WorkspaceRef,
    pub created_at: rein_core::time::Timestamp,
    /// Execution binding default (C2 amendment: not semantic content).
    #[serde(default)]
    pub default_hand: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub rein_dir: PathBuf,
    pub manifest: WorkspaceManifest,
}

impl Workspace {
    pub fn dir(root: &Path) -> PathBuf {
        root.join(".rein")
    }

    pub fn ledger_db(&self) -> PathBuf {
        self.rein_dir.join("ledger.db")
    }

    pub fn objects(&self) -> PathBuf {
        self.rein_dir.join("objects")
    }

    pub fn tmp(&self) -> PathBuf {
        self.rein_dir.join("tmp")
    }

    pub fn logs(&self) -> PathBuf {
        self.rein_dir.join("logs")
    }

    pub fn plans(&self) -> PathBuf {
        self.rein_dir.join("plans")
    }

    pub fn skills(&self) -> PathBuf {
        self.rein_dir.join("skills")
    }

    pub fn policies(&self) -> PathBuf {
        self.rein_dir.join("policies")
    }

    pub fn cache(&self) -> PathBuf {
        self.rein_dir.join("cache")
    }

    pub fn providers_lock(&self) -> PathBuf {
        self.rein_dir.join("providers.lock")
    }

    fn manifest_path(rein_dir: &Path) -> PathBuf {
        rein_dir.join("workspace.yaml")
    }

    /// `rein init`: create the full §11 layout. Refuses an existing workspace.
    pub fn init(
        root: &Path,
        workspace_ref: WorkspaceRef,
        now: rein_core::time::Timestamp,
    ) -> Result<Self, WorkspaceError> {
        let rein_dir = Self::dir(root);
        if rein_dir.exists() {
            return Err(WorkspaceError::AlreadyExists(root.to_path_buf()));
        }
        for sub in [
            "", "objects", "cache", "logs", "tmp", "plans", "skills", "policies",
        ] {
            let p = rein_dir.join(sub);
            std::fs::create_dir_all(&p).map_err(io_err(&p))?;
        }
        let manifest = WorkspaceManifest {
            schema: WORKSPACE_SCHEMA.to_string(),
            workspace_ref,
            created_at: now,
            default_hand: Some("fake:deterministic-a".to_string()),
        };
        let text = serde_yaml::to_string(&manifest)
            .map_err(|e| WorkspaceError::Manifest(e.to_string()))?;
        let mp = Self::manifest_path(&rein_dir);
        std::fs::write(&mp, text).map_err(io_err(&mp))?;
        Ok(Self {
            root: root.to_path_buf(),
            rein_dir,
            manifest,
        })
    }

    pub fn open(root: &Path) -> Result<Self, WorkspaceError> {
        let rein_dir = Self::dir(root);
        let mp = Self::manifest_path(&rein_dir);
        let text = std::fs::read_to_string(&mp)
            .map_err(|_| WorkspaceError::NotFound(root.to_path_buf()))?;
        let manifest: WorkspaceManifest =
            serde_yaml::from_str(&text).map_err(|e| WorkspaceError::Manifest(e.to_string()))?;
        if manifest.schema != WORKSPACE_SCHEMA {
            return Err(WorkspaceError::Manifest(format!(
                "schema is `{}`, expected `{WORKSPACE_SCHEMA}`",
                manifest.schema
            )));
        }
        Ok(Self {
            root: root.to_path_buf(),
            rein_dir,
            manifest,
        })
    }

    /// Walk upward from `start` to the nearest workspace.
    pub fn discover(start: &Path) -> Result<Self, WorkspaceError> {
        let mut cur = Some(start.to_path_buf());
        while let Some(dir) = cur {
            if Self::dir(&dir).exists() {
                return Self::open(&dir);
            }
            cur = dir.parent().map(Path::to_path_buf);
        }
        Err(WorkspaceError::NotFound(start.to_path_buf()))
    }
}

/// User-level configuration (configRoot), layered under flags/env per §9.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserConfig {
    #[serde(default)]
    pub default_hand: Option<String>,
    #[serde(default)]
    pub searxng_url: Option<String>,
    #[serde(default)]
    pub fmp_key_ref: Option<String>,
    #[serde(default)]
    pub agora_key_path: Option<String>,
    #[serde(default)]
    pub agy_path: Option<String>,
    #[serde(default)]
    pub agy_model: Option<String>,
    /// An operator-named env file to fall back to for provider keys —
    /// a pointer, never a value.
    #[serde(default)]
    pub fmp_env_file: Option<String>,
    /// AGORA hub base URL for `evidence publish`. No baked-in default:
    /// unset and no `--hub` means the crossing refuses with a stated reason.
    #[serde(default)]
    pub agora_hub: Option<String>,
}

pub fn default_config_root() -> PathBuf {
    std::env::var_os("REIN_CONFIG_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".config").join("rein")
        })
}

pub fn load_user_config(config_root: &Path) -> UserConfig {
    let path = config_root.join("config.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return UserConfig::default();
    };
    // Minimal TOML subset: `key = "value"` lines. Avoids a toml dependency
    // for five keys; anything richer belongs in a later milestone.
    let mut cfg = UserConfig::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        match k.trim() {
            "default_hand" => cfg.default_hand = Some(v),
            "searxng_url" => cfg.searxng_url = Some(v),
            "fmp_key_ref" => cfg.fmp_key_ref = Some(v),
            "agora_key_path" => cfg.agora_key_path = Some(v),
            "agy_path" => cfg.agy_path = Some(v),
            "agy_model" => cfg.agy_model = Some(v),
            "fmp_env_file" => cfg.fmp_env_file = Some(v),
            "agora_hub" => cfg.agora_hub = Some(v),
            _ => {}
        }
    }
    cfg
}

/// Secrets: names → values, resolved from configRoot ONLY (invariant 27).
/// Values are held transiently for injection and redaction; durable state
/// carries only `secret-ref:` names.
pub struct SecretBroker {
    entries: Vec<(SecretRefId, String)>,
}

impl SecretBroker {
    /// Open the broker. Refuses a config root inside the workspace root — the
    /// mutation test for invariant 27 lives on this line.
    pub fn open(config_root: &Path, workspace_root: &Path) -> Result<Self, WorkspaceError> {
        let c = config_root
            .canonicalize()
            .unwrap_or_else(|_| config_root.to_path_buf());
        let w = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        if c.starts_with(&w) {
            return Err(WorkspaceError::ConfigInsideWorkspace {
                config: c,
                workspace: w,
            });
        }
        let mut entries = Vec::new();
        let path = c.join("secrets.toml");
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.starts_with('[') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let name = k.trim();
                    let value = v.trim().trim_matches('"').to_string();
                    if let Ok(id) = SecretRefId::parse(&format!("secret-ref:{name}")) {
                        entries.push((id, value));
                    }
                }
            }
        }
        Ok(Self { entries })
    }

    pub fn resolve(&self, id: &SecretRefId) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
    }

    pub fn known_refs(&self) -> Vec<SecretRefId> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    /// The redactor over every known secret value (invariant 28).
    pub fn redactor(&self) -> Redactor {
        Redactor::new(self.entries.clone())
    }

    /// Environment map for a grant's secret refs (narrowest boundary: only
    /// the refs the grant names, only at spawn time).
    pub fn env_for(&self, refs: &[SecretRefId]) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        for r in refs {
            if let Some(v) = self.resolve(r) {
                let name = r
                    .as_str()
                    .trim_start_matches("secret-ref:")
                    .to_uppercase()
                    .replace('-', "_");
                env.insert(name, v.to_string());
            }
        }
        env
    }
}
