use std::{collections::HashSet, path::PathBuf, sync::Arc};

use rust_extensions::AppStates;

use crate::{
    audit::AuditLog,
    repo::{expand_home, RepoContext},
    settings::SettingsModel,
};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");

pub struct AppContext {
    pub app_states: Arc<AppStates>,
    pub repos: Vec<Arc<RepoContext>>,
    pub bind_addr: String,
    pub auth_token: String,
}

impl AppContext {
    /// Everything the settings describe is turned into live objects here, at
    /// startup, and validated on the way. A bad repository root or a duplicated
    /// endpoint should stop the server coming up — not surface later as an
    /// endpoint that fails every call.
    pub async fn new(settings: SettingsModel) -> Result<Self, String> {
        if settings.auth_token.trim().is_empty() {
            return Err("auth_token is empty. The server refuses to serve without one".to_string());
        }

        if settings.repos.is_empty() {
            return Err("No repositories are configured, so there is nothing to serve".to_string());
        }

        // The journal is opt-in: a path turns it on, its absence leaves it off.
        let audit = Arc::new(match settings.audit_log_path.as_ref() {
            Some(path) => AuditLog::new(PathBuf::from(expand_home(path))),
            None => AuditLog::disabled(),
        });

        let mut repos = Vec::with_capacity(settings.repos.len());
        let mut used_paths = HashSet::new();

        for repo_settings in settings.repos.iter() {
            let repo = RepoContext::new(&settings, repo_settings, audit.clone()).await?;

            // Paths are matched case-insensitively by the MCP middleware, so
            // two repos differing only in case would leave one unreachable.
            if !used_paths.insert(repo.mcp_path.to_lowercase()) {
                return Err(format!(
                    "mcp_path '{}' is used by more than one repository",
                    repo.mcp_path
                ));
            }

            repos.push(Arc::new(repo));
        }

        Ok(Self {
            app_states: Arc::new(AppStates::create_initialized()),
            repos,
            bind_addr: settings.bind_addr.clone(),
            // Trimmed to match the presented token, which `strip_bearer` trims.
            // Storing it untrimmed would let a token with stray whitespace pass
            // startup validation and then reject every request — including one
            // carrying the byte-exact configured value.
            auth_token: settings.auth_token.trim().to_string(),
        })
    }
}
