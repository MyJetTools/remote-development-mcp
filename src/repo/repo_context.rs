use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    activity::ActivityLog,
    audit::AuditLog,
    jobs::JobsRegistry,
    settings::{RepoSettings, SettingsModel},
};

use super::{resolve_inside_root, CommandPolicy};

/// Everything one repository endpoint is allowed to touch.
///
/// Each MCP endpoint gets its own `RepoContext`, and its tool handlers hold
/// nothing else. That is what makes the isolation structural rather than a
/// matter of validating a `repo` argument on every call: a handler mounted at
/// `/my-ssh` has no reference to any other repository's root to begin with.
pub struct RepoContext {
    /// `McpMiddleware::new` wants a `&'static str`, and these paths come from a
    /// config file, so the string is leaked once here at startup. Bounded: it
    /// happens exactly once per configured repository, never per request.
    pub mcp_path: &'static str,

    /// Short name used in the audit trail and as the log folder name.
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
    pub logs_dir: PathBuf,
    pub default_timeout_sec: u64,
    pub max_log_bytes: u64,
    pub allow_delete: bool,
}

impl RepoContext {
    pub async fn new(
        settings: &SettingsModel,
        repo: &RepoSettings,
        audit: Arc<AuditLog>,
        activity: Arc<ActivityLog>,
    ) -> Result<Self, String> {
        let mcp_path = repo.mcp_path.trim().to_string();

        if !mcp_path.starts_with('/') {
            return Err(format!(
                "Repository mcp_path '{}' must start with '/'",
                mcp_path
            ));
        }

        let root = expand_home(&repo.root);

        // Fail at startup rather than serving an endpoint whose every call would
        // fail: a mistyped root is a configuration error, not a runtime one.
        let root = PathBuf::from(&root).canonicalize().map_err(|err| {
            format!(
                "Repository root '{}' (mcp_path '{}') can not be resolved. Err: {}",
                repo.root, mcp_path, err
            )
        })?;

        if !root.is_dir() {
            return Err(format!(
                "Repository root '{}' (mcp_path '{}') is not a directory",
                root.display(),
                mcp_path
            ));
        }

        let name = name_from_mcp_path(&mcp_path);

        let logs_dir = PathBuf::from(expand_home(&settings.logs_path)).join(&name);

        tokio::fs::create_dir_all(&logs_dir).await.map_err(|err| {
            format!(
                "Can not create the job logs folder '{}'. Err: {}",
                logs_dir.display(),
                err
            )
        })?;

        let command_mode = match repo.command_mode {
            Some(command_mode) => command_mode,
            None => settings.command_mode,
        };

        let command_allowlist = match repo.command_allowlist.as_ref() {
            Some(command_allowlist) => command_allowlist.clone(),
            None => settings.command_allowlist.clone(),
        };

        Ok(Self {
            mcp_path: Box::leak(mcp_path.into_boxed_str()),
            name,
            description: repo.description.clone(),
            root,
            command_policy: CommandPolicy::new(command_mode, command_allowlist),
            jobs: JobsRegistry::new(settings.max_concurrent_jobs),
            audit,
            activity,
            logs_dir,
            default_timeout_sec: settings.default_timeout_sec,
            max_log_bytes: settings.max_log_bytes,
            allow_delete: repo.allow_delete,
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

/// `/my-ssh` -> `my-ssh`, `/group/my-ssh` -> `group_my-ssh`. Used as a folder
/// name, so it must not reintroduce path separators.
fn name_from_mcp_path(mcp_path: &str) -> String {
    let name = mcp_path.trim_matches('/').replace('/', "_");

    if name.is_empty() {
        "root".to_string()
    } else {
        name
    }
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
    fn derives_a_filesystem_safe_name() {
        assert_eq!(name_from_mcp_path("/my-ssh"), "my-ssh");
        assert_eq!(name_from_mcp_path("/group/my-ssh"), "group_my-ssh");
        assert_eq!(name_from_mcp_path("/"), "root");
    }

    async fn test_repo(name: &str) -> Arc<RepoContext> {
        use crate::{
            audit::AuditLog,
            settings::{CommandMode, RepoSettings, SettingsModel},
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
            repos: Vec::new(),
            command_mode: CommandMode::Allowlist,
            command_allowlist: Vec::new(),
            max_concurrent_jobs: 1,
            default_timeout_sec: 60,
            max_log_bytes: 1024,
            logs_path: base.join("logs").to_string_lossy().to_string(),
            audit_log_path: None,
        };

        let repo_settings = RepoSettings {
            mcp_path: format!("/{}", name),
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
                &repo_settings,
                audit,
                std::sync::Arc::new(crate::activity::ActivityLog::new(false)),
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
