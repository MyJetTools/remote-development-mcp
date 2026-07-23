use std::{collections::HashSet, path::PathBuf, sync::Arc};

use ahash::AHashMap;
use rust_extensions::AppStates;

use crate::{
    actions::WatchedRuns,
    activity::ActivityLog,
    audit::AuditLog,
    repo::{expand_home, Endpoint, RepoContext},
    sessions::SessionsRegistry,
    settings::SettingsModel,
};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");

pub struct AppContext {
    pub app_states: Arc<AppStates>,
    /// When the process came up — the console shows uptime.
    pub started_at: rust_extensions::date_time::DateTimeAsMicroseconds,
    /// Every project this machine serves, built once and shared by whichever
    /// endpoints expose it.
    pub projects: Vec<Arc<RepoContext>>,
    /// The MCP URLs, each a view over some of [`Self::projects`].
    pub endpoints: Vec<Arc<Endpoint>>,
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

        if settings.projects.is_empty() {
            return Err("No projects are configured, so there is nothing to serve".to_string());
        }

        if settings.endpoints.is_empty() {
            return Err(
                "No endpoints are configured, so the projects are not reachable. Add at least one \
                 endpoint listing the projects it exposes"
                    .to_string(),
            );
        }

        // The journal is opt-in: a path turns it on, its absence leaves it off.
        let audit = Arc::new(match settings.audit_log_path.as_ref() {
            Some(path) => AuditLog::new(PathBuf::from(expand_home(path))),
            None => AuditLog::disabled(),
        });

        let mut projects = Vec::with_capacity(settings.projects.len());
        let mut by_id: AHashMap<String, Arc<RepoContext>> = AHashMap::new();

        for project_settings in settings.projects.iter() {
            let project = Arc::new(
                RepoContext::new(
                    &settings,
                    project_settings,
                    audit.clone(),
                    activity.clone(),
                    watched_runs.clone(),
                )
                .await?,
            );

            // Ids key the log folder and the job-id prefix, so a duplicate would
            // silently merge two repositories' jobs and logs.
            if by_id
                .insert(project.name.clone(), project.clone())
                .is_some()
            {
                return Err(format!(
                    "Project id '{}' is used by more than one project",
                    project.name
                ));
            }

            projects.push(project);
        }

        let mut endpoints = Vec::with_capacity(settings.endpoints.len());
        let mut used_urls = HashSet::new();

        for endpoint_settings in settings.endpoints.iter() {
            let mut exposed = Vec::with_capacity(endpoint_settings.projects.len());

            for id in endpoint_settings.projects.iter() {
                let project = by_id.get(id.trim()).ok_or_else(|| {
                    format!(
                        "Endpoint '{}' lists project '{}', which is not configured",
                        endpoint_settings.url, id
                    )
                })?;

                exposed.push(project.clone());
            }

            let endpoint = Endpoint::new(
                &endpoint_settings.url,
                endpoint_settings.description.clone(),
                exposed,
            )?;

            // Urls are matched case-insensitively by the MCP middleware, so two
            // endpoints differing only in case would leave one unreachable.
            if !used_urls.insert(endpoint.url.to_lowercase()) {
                return Err(format!(
                    "url '{}' is used by more than one endpoint",
                    endpoint.url
                ));
            }

            endpoints.push(Arc::new(endpoint));
        }

        Ok(Self {
            app_states: Arc::new(AppStates::create_initialized()),
            started_at: rust_extensions::date_time::DateTimeAsMicroseconds::now(),
            projects,
            endpoints,
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
