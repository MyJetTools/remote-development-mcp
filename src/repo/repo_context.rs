use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    activity::ActivityLog,
    audit::AuditLog,
    jobs::JobsRegistry,
    settings::{ProjectSettings, SettingsModel},
};

use super::{resolve_inside_root, CommandPolicy};

/// One project: everything tools are allowed to touch inside a single
/// repository.
///
/// A project is built once per machine and shared by every endpoint that
/// exposes it, which is deliberate — two URLs onto the same repository must see
/// the same jobs and share its concurrency limit, otherwise the limit is dodged
/// by connecting twice. Which projects a caller can name at all is decided by
/// the endpoint they reached; see [`super::Endpoint`].
pub struct RepoContext {
    /// The `id` from the settings. Used as the job-id prefix, the log folder
    /// name, the audit key and the label the console shows.
    ///
    /// Not the GitHub repository name — that lives on `WatchedRun`.
    pub name: String,

    /// Free-form note from the settings, shown to the client as part of the
    /// endpoint instructions.
    pub description: Option<String>,

    root: PathBuf,

    pub command_policy: CommandPolicy,
    pub jobs: JobsRegistry,
    pub audit: Arc<AuditLog>,
    /// Feed the console renders — every tool call and job completion lands here.
    pub activity: Arc<ActivityLog>,
    /// GitHub Actions runs being followed. Shared with the poller and the
    /// console, so all three see one state.
    pub watched_runs: Arc<crate::actions::WatchedRuns>,
    pub logs_dir: PathBuf,
    pub default_timeout_sec: u64,
    pub max_log_bytes: u64,
    pub allow_delete: bool,
    /// Token for the GitHub REST API, from the settings. `None` disables
    /// `create_release`.
    pub github_token: Option<String>,
}

impl RepoContext {
    pub async fn new(
        settings: &SettingsModel,
        project: &ProjectSettings,
        audit: Arc<AuditLog>,
        activity: Arc<ActivityLog>,
        watched_runs: Arc<crate::actions::WatchedRuns>,
    ) -> Result<Self, String> {
        let name = validate_id(&project.id)?;

        let root = expand_home(&project.root);

        // Fail at startup rather than serving a project whose every call would
        // fail: a mistyped root is a configuration error, not a runtime one.
        let root = PathBuf::from(&root).canonicalize().map_err(|err| {
            format!(
                "Root '{}' of project '{}' can not be resolved. Err: {}",
                project.root, name, err
            )
        })?;

        if !root.is_dir() {
            return Err(format!(
                "Root '{}' of project '{}' is not a directory",
                root.display(),
                name
            ));
        }

        let logs_dir = PathBuf::from(expand_home(&settings.logs_path)).join(&name);

        tokio::fs::create_dir_all(&logs_dir).await.map_err(|err| {
            format!(
                "Can not create the job logs folder '{}'. Err: {}",
                logs_dir.display(),
                err
            )
        })?;

        let command_mode = match project.command_mode {
            Some(command_mode) => command_mode,
            None => settings.command_mode,
        };

        let command_allowlist = match project.command_allowlist.as_ref() {
            Some(command_allowlist) => command_allowlist.clone(),
            None => settings.command_allowlist.clone(),
        };

        Ok(Self {
            jobs: JobsRegistry::new(settings.max_concurrent_jobs, name.clone()),
            name,
            description: project.description.clone(),
            root,
            command_policy: CommandPolicy::new(command_mode, command_allowlist),
            audit,
            activity,
            watched_runs,
            logs_dir,
            default_timeout_sec: settings.default_timeout_sec,
            max_log_bytes: settings.max_log_bytes,
            allow_delete: project.allow_delete,
            github_token: settings.github_token.clone(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The only way any tool is allowed to turn a caller-supplied string into a
    /// path.
    pub fn resolve_path(&self, requested: &str) -> Result<PathBuf, String> {
        resolve_inside_root(&self.root, requested)
    }

    /// Resolves a path the caller intends to *write*, and additionally refuses
    /// anything inside a `.git` directory.
    ///
    /// This is a real privilege boundary, not tidiness: git executes several of
    /// the files under `.git` — `core.fsmonitor` and `core.hooksPath` in
    /// `.git/config`, the hooks themselves — so a caller able to write there
    /// would get code execution through the server's own `git status` /
    /// `git apply`, with no `run_command`, no allowlist check, and no audit
    /// record. Reads are unaffected; only writes are closed off.
    pub fn resolve_writable_path(&self, requested: &str) -> Result<PathBuf, String> {
        let resolved = self.resolve_path(requested)?;

        if self.is_inside_git_dir(&resolved) {
            return Err(format!(
                "Refusing to write inside a '.git' directory ('{}'). Git runs some of the files \
                 there, so the tools never modify git's own metadata",
                self.to_relative(&resolved)
            ));
        }

        Ok(resolved)
    }

    /// Resolves a path whose final component must be treated as itself, even if
    /// it is a symlink — for `delete_path` and `move_path`, which operate on the
    /// link, not its target.
    ///
    /// The parent directory is confined the usual way (symlinks and `..` in it
    /// are resolved and checked against the root), but the last component is
    /// appended literally and never followed. That deletes or renames a symlink
    /// as the link it is, rather than reaching through it to a file elsewhere,
    /// and it also makes a dangling link removable — only its parent has to
    /// resolve inside the root, not the broken target.
    ///
    /// Also refuses `.git`, like [`Self::resolve_writable_path`].
    pub fn resolve_symlink_path(&self, requested: &str) -> Result<PathBuf, String> {
        let requested = requested.trim();
        let as_path = Path::new(requested);

        let leaf = match as_path.file_name() {
            Some(leaf) => leaf,
            // No final component to keep — `.`, `..`, `/`, or a trailing slash.
            None => {
                return Err(format!(
                    "Path '{}' does not name a file or directory that can be removed or moved",
                    requested
                ))
            }
        };

        let parent = match as_path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                self.resolve_path(&parent.to_string_lossy())?
            }
            _ => self.root.clone(),
        };

        let resolved = parent.join(leaf);

        // `leaf` is a single normal component and `parent` is confined, so this
        // holds; verified because the next thing a caller does is delete it.
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "Path '{}' resolves outside of the repository root and was refused",
                requested
            ));
        }

        if self.is_inside_git_dir(&resolved) {
            return Err(format!(
                "Refusing to touch a '.git' directory ('{}')",
                self.to_relative(&resolved)
            ));
        }

        Ok(resolved)
    }

    /// True when any component of the path, relative to the root, is `.git` —
    /// covering a nested repository's `.git` as well as the top-level one.
    fn is_inside_git_dir(&self, path: &Path) -> bool {
        let relative = match path.strip_prefix(&self.root) {
            Ok(relative) => relative,
            // resolve_path guarantees this is under the root; if that ever fails
            // to hold, refusing is the safe direction.
            Err(_) => return true,
        };

        relative
            .components()
            .any(|component| component.as_os_str() == ".git")
    }

    /// Renders a resolved path back the way the caller thinks about it —
    /// relative to the repository root — so responses never leak the layout of
    /// the host filesystem.
    pub fn to_relative(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(relative) => {
                let relative = relative.to_string_lossy().to_string();

                if relative.is_empty() {
                    ".".to_string()
                } else {
                    relative
                }
            }
            Err(_) => path.to_string_lossy().to_string(),
        }
    }
}

/// Expands a leading `~`, and only a leading one.
///
/// `rust_extensions::file_utils::format_path` would do this too, but it
/// replaces *every* `~` in the string, so a legitimate path like
/// `/data/backup~old` comes back mangled. A repository root is worth being
/// literal about.
pub fn expand_home(src: &str) -> String {
    let src = src.trim();

    if src != "~" && !src.starts_with("~/") {
        return src.to_string();
    }

    let home = match std::env::var("HOME") {
        Ok(home) => home,
        Err(_) => return src.to_string(),
    };

    if src == "~" {
        return home;
    }

    format!("{}/{}", home.trim_end_matches('/'), &src[2..])
}

/// A project id is not decoration: it is joined onto `logs_path` as a directory
/// name and it prefixes every job id, so it is restricted to characters that can
/// not turn either into something else.
///
/// `/` and `..` would let a mistyped id write logs outside `logs_path`, and `:`
/// is what separates the project from the job number inside a job id — an id
/// containing one would make job ids ambiguous to parse.
fn validate_id(id: &str) -> Result<String, String> {
    let id = id.trim();

    if id.is_empty() {
        return Err("A project id can not be empty".to_string());
    }

    let allowed = id
        .chars()
        .all(|itm| itm.is_ascii_alphanumeric() || itm == '.' || itm == '_' || itm == '-');

    if !allowed {
        return Err(format!(
            "Project id '{}' may only contain letters, digits, '.', '_' and '-'. It becomes a \
             folder name under logs_path and the prefix of every job id",
            id
        ));
    }

    // Allowed by the charset above, but it still names the parent directory.
    if id == "." || id == ".." {
        return Err(format!("Project id '{}' is not a usable folder name", id));
    }

    Ok(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_only_a_leading_tilde() {
        let home = std::env::var("HOME").unwrap();

        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/projects"), format!("{}/projects", home));

        // The case `format_path` gets wrong.
        assert_eq!(expand_home("/data/backup~old"), "/data/backup~old");
        assert_eq!(expand_home("/absolute/path"), "/absolute/path");
    }

    #[test]
    fn rejects_ids_that_would_escape_the_logs_folder() {
        assert_eq!(validate_id("my-ssh").unwrap(), "my-ssh");
        assert_eq!(validate_id(" my-ssh ").unwrap(), "my-ssh");

        // Would write logs outside logs_path.
        assert!(validate_id("../etc").is_err());
        assert!(validate_id("group/my-ssh").is_err());
        assert!(validate_id("..").is_err());

        // Would make a job id ambiguous to split.
        assert!(validate_id("my:ssh").is_err());

        assert!(validate_id("").is_err());
        assert!(validate_id("   ").is_err());
    }

    async fn test_repo(name: &str) -> Arc<RepoContext> {
        use crate::{
            audit::AuditLog,
            settings::{CommandMode, ProjectSettings, SettingsModel},
        };

        let base = std::env::temp_dir()
            .join("remote-development-mcp-tests-writable")
            .join(name);

        let _ = std::fs::remove_dir_all(&base);

        let root = base.join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let settings = SettingsModel {
            bind_addr: "127.0.0.1:0".to_string(),
            auth_token: Some("t".to_string()),
            projects: Vec::new(),
            endpoints: Vec::new(),
            command_mode: CommandMode::Allowlist,
            command_allowlist: Vec::new(),
            max_concurrent_jobs: 1,
            default_timeout_sec: 60,
            max_log_bytes: 1024,
            logs_path: base.join("logs").to_string_lossy().to_string(),
            audit_log_path: None,
            github_token: None,
        };

        let project_settings = ProjectSettings {
            id: name.to_string(),
            root: root.to_string_lossy().to_string(),
            description: None,
            command_mode: None,
            command_allowlist: None,
            allow_delete: false,
        };

        let audit = Arc::new(AuditLog::disabled());

        Arc::new(
            RepoContext::new(
                &settings,
                &project_settings,
                audit,
                std::sync::Arc::new(crate::activity::ActivityLog::new()),
                std::sync::Arc::new(crate::actions::WatchedRuns::new()),
            )
            .await
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn writing_inside_dot_git_is_refused() {
        let repo = test_repo("git_write_block").await;

        // The privilege-escalation path: plant config git later executes.
        let err = repo.resolve_writable_path(".git/config").unwrap_err();
        assert!(err.contains(".git"), "{}", err);

        assert!(repo.resolve_writable_path(".git/hooks/pre-commit").is_err());

        // A normal source file is still writable.
        assert!(repo.resolve_writable_path("src/main.rs").is_ok());
    }

    #[tokio::test]
    async fn reading_inside_dot_git_is_still_allowed() {
        let repo = test_repo("git_read_ok").await;

        // Only writes are closed off; resolve_path (reads) is unaffected.
        assert!(repo.resolve_path(".git/config").is_ok());
    }
}
