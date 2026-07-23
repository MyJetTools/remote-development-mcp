use std::{collections::HashSet, path::PathBuf, sync::Arc};

use rust_extensions::AppStates;

use crate::{
    actions::WatchedRuns,
    activity::ActivityLog,
    audit::AuditLog,
    repo::{expand_home, RepoContext},
    sessions::SessionsRegistry,
    settings::SettingsModel,
};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");

pub struct AppContext {
    pub app_states: Arc<AppStates>,
    /// When the process came up — the console shows uptime.
    pub started_at: rust_extensions::date_time::DateTimeAsMicroseconds,
    pub repos: Vec<Arc<RepoContext>>,
    pub bind_addr: String,
    /// What the browser console renders. In-memory only — nothing here is
    /// written to the terminal or survives a restart.
    pub activity: Arc<ActivityLog>,
    /// GitHub Actions runs being followed, shared with the poller and the
    /// REST surface.
    pub watched_runs: Arc<WatchedRuns>,
    /// Live MCP sessions. Filled by the middleware's lifecycle hooks, so it
    /// holds what is connected now rather than a guess from request traffic.
    pub sessions: Arc<SessionsRegistry>,
    /// `None` means the server does not authenticate at all and trusts whatever
    /// reaches it — the normal setup, where a reverse proxy in front terminates
    /// authentication.
    pub auth_token: Option<String>,
}

impl AppContext {
    /// Everything the settings describe is turned into live objects here, at
    /// startup, and validated on the way. A bad repository root or a duplicated
    /// endpoint should stop the server coming up — not surface later as an
    /// endpoint that fails every call.
    pub async fn new(settings: SettingsModel, activity: Arc<ActivityLog>) -> Result<Self, String> {
        let watched_runs = Arc::new(WatchedRuns::new());
        let sessions = Arc::new(SessionsRegistry::new());

        // No token configured means no authentication — see the field's note. A
        // token that is present but blank is a mistake worth catching, though,
        // since it reads as "protected" while protecting nothing.
        let auth_token =
            match settings.auth_token.as_ref() {
                Some(token) if token.trim().is_empty() => return Err(
                    "auth_token is present but blank. Remove it to run without authentication, \
                     or set a real token"
                        .to_string(),
                ),
                Some(token) => Some(token.trim().to_string()),
                None => None,
            };

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
            let repo = RepoContext::new(
                &settings,
                repo_settings,
                audit.clone(),
                activity.clone(),
                watched_runs.clone(),
            )
            .await?;

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
            started_at: rust_extensions::date_time::DateTimeAsMicroseconds::now(),
            repos,
            bind_addr: settings.bind_addr.clone(),
            activity,
            watched_runs,
            sessions,
            // Already trimmed above, to match the presented token — which
            // `strip_bearer` trims. Storing it untrimmed would let a token with
            // stray whitespace pass startup validation and then reject every
            // request, including one carrying the byte-exact configured value.
            auth_token,
        })
    }
}
