use serde::{Deserialize, Serialize};

/// Settings are read once at startup. Everything they describe — the set of
/// repositories, their MCP endpoints and their command policies — is baked
/// into the middleware graph while the HTTP server is being built, so a change
/// to this file takes effect on the next restart.
#[derive(my_settings_reader::SettingsModel, Serialize, Deserialize, Debug, Clone)]
pub struct SettingsModel {
    /// Where the HTTP server listens. Keep it on the loopback interface and
    /// publish it through a tunnel — never expose the port directly.
    pub bind_addr: String,

    /// Bearer token every request must carry.
    ///
    /// Optional, and normally left unset: this server is meant to sit behind a
    /// reverse proxy that terminates authentication, so it trusts whatever
    /// reaches it. Set a token only when the port is reachable by something
    /// other than that proxy.
    #[serde(default)]
    pub auth_token: Option<String>,

    /// Repositories to serve. Each one becomes its own MCP endpoint.
    pub repos: Vec<RepoSettings>,

    /// Default command policy for repos that do not override it.
    #[serde(default)]
    pub command_mode: CommandMode,

    /// Default allowlist for repos that do not override it.
    #[serde(default = "default_command_allowlist")]
    pub command_allowlist: Vec<String>,

    /// How many jobs may run at once inside a single repository.
    #[serde(default = "default_max_concurrent_jobs")]
    pub max_concurrent_jobs: usize,

    /// Applied to jobs that do not request a timeout of their own.
    #[serde(default = "default_timeout_sec")]
    pub default_timeout_sec: u64,

    /// Per-stream cap on a job log. Once a stream reaches it the job keeps
    /// running but its log stops growing and is reported as truncated.
    #[serde(default = "default_max_log_bytes")]
    pub max_log_bytes: u64,

    /// Directory holding per-job stdout/stderr logs.
    #[serde(default = "default_logs_path")]
    pub logs_path: String,

    /// Append-only audit trail of every command that was started. Off unless a
    /// path is set here — set one to turn the journal on.
    #[serde(default)]
    pub audit_log_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RepoSettings {
    /// MCP endpoint this repository is served at, e.g. `/my-ssh`. Matched
    /// case-insensitively and in full, so every repo needs a distinct value.
    pub mcp_path: String,

    /// Repository root. `~` is expanded. Every path argument of every tool is
    /// resolved against it and refused if it lands outside.
    pub root: String,

    /// Shown to the MCP client as the server instructions for this endpoint.
    #[serde(default)]
    pub description: Option<String>,

    /// Overrides [`SettingsModel::command_mode`] for this repository.
    #[serde(default)]
    pub command_mode: Option<CommandMode>,

    /// Overrides [`SettingsModel::command_allowlist`] for this repository.
    #[serde(default)]
    pub command_allowlist: Option<Vec<String>>,

    /// `delete_path` is refused unless this is turned on.
    #[serde(default)]
    pub allow_delete: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandMode {
    /// Only binaries named in the allowlist may be started.
    #[default]
    Allowlist,
    /// Any binary may be started, still confined to the repository root.
    Passthrough,
}

/// Kept to the build-and-inspect tools that have no confined equivalent.
///
/// Deliberately excluded:
/// - `sh` / `bash` — a shell in the allowlist is passthrough in one step.
/// - `cat` / `ls` / `mkdir` / `mv` — general file-I/O binaries whose arguments
///   are not confined, so `cat /etc/…` would read outside the repository. The
///   `read_file`, `list_dir`, `write_file` and `move_path` tools cover these and
///   are confined to the root.
/// - `rustup` — `rustup run <toolchain> <program>` executes any binary; toolchain
///   management is an operator action, not a client one.
///
/// An operator who wants any of these can add them explicitly, or switch the
/// repository to `command_mode: passthrough`.
fn default_command_allowlist() -> Vec<String> {
    ["cargo", "rustc", "git", "rg"]
        .iter()
        .map(|itm| itm.to_string())
        .collect()
}

fn default_max_concurrent_jobs() -> usize {
    4
}

fn default_timeout_sec() -> u64 {
    3600
}

fn default_max_log_bytes() -> u64 {
    16 * 1024 * 1024
}

fn default_logs_path() -> String {
    "~/.remote-development-mcp-logs".to_string()
}
