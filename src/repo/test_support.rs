//! Shared helpers for building a throwaway [`RepoContext`] in tests.

use std::sync::Arc;

use crate::{
    audit::AuditLog,
    settings::{CommandMode, RepoSettings, SettingsModel},
};

use super::RepoContext;

pub struct TestRepoOptions {
    pub command_mode: CommandMode,
    pub command_allowlist: Vec<String>,
    pub allow_delete: bool,
    /// When set, the audit journal is enabled and written to this path.
    pub audit_path: Option<std::path::PathBuf>,
}

impl Default for TestRepoOptions {
    fn default() -> Self {
        Self {
            command_mode: CommandMode::Allowlist,
            command_allowlist: Vec::new(),
            allow_delete: false,
            audit_path: None,
        }
    }
}

/// Builds a `RepoContext` rooted at a fresh temp directory. The returned root is
/// the repository root; create fixtures under it with `repo.root()`.
pub async fn build_test_repo(name: &str, options: TestRepoOptions) -> Arc<RepoContext> {
    let base = std::env::temp_dir()
        .join("remote-development-mcp-tests")
        .join(name);

    let _ = std::fs::remove_dir_all(&base);

    let root = base.join("repo");
    std::fs::create_dir_all(&root).unwrap();

    let settings = SettingsModel {
        bind_addr: "127.0.0.1:0".to_string(),
        auth_token: Some("test-token".to_string()),
        repos: Vec::new(),
        command_mode: options.command_mode,
        command_allowlist: options.command_allowlist,
        max_concurrent_jobs: 4,
        default_timeout_sec: 60,
        max_log_bytes: 1024 * 1024,
        logs_path: base.join("logs").to_string_lossy().to_string(),
        audit_log_path: None,
    };

    let repo_settings = RepoSettings {
        mcp_path: format!("/{}", name),
        root: root.to_string_lossy().to_string(),
        description: None,
        command_mode: None,
        command_allowlist: None,
        allow_delete: options.allow_delete,
    };

    let audit = Arc::new(match options.audit_path {
        Some(path) => AuditLog::new(path),
        None => AuditLog::disabled(),
    });

    Arc::new(
        RepoContext::new(&settings, &repo_settings, audit)
            .await
            .unwrap(),
    )
}
