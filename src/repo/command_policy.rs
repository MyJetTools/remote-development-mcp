use crate::settings::CommandMode;

/// Decides which binaries a repository is allowed to start, and with which
/// environment.
///
/// This is a guardrail, not a sandbox, and it is worth being honest about where
/// the line is:
///
/// - `cargo` runs `build.rs` and `rustc` runs proc-macros, so an allowlisted
///   build is arbitrary code execution by design.
/// - `run_command` **arguments are not path-confined**. The path confinement in
///   [`super::path_confinement`] governs the file tools and a command's `cwd`,
///   but not the strings passed to the process — `git show HEAD:foo` reads a
///   tracked file, and a binary that takes an absolute path would act on it.
///   This is why the default allowlist is kept to build-and-inspect tools with
///   no general file-I/O binaries in it, and why the environment is filtered.
///
/// What the policy does buy: a caller can not reach for an arbitrary binary by
/// name, can not redirect an allowlisted name to another file through the
/// environment (see [`Self::check_env`]), and every started process is recorded
/// in the audit log. Run this only against repositories whose contents you
/// would already run locally.
pub struct CommandPolicy {
    mode: CommandMode,
    allowlist: Vec<String>,
}

impl CommandPolicy {
    pub fn new(mode: CommandMode, allowlist: Vec<String>) -> Self {
        Self { mode, allowlist }
    }

    pub fn check(&self, command: &str) -> Result<(), String> {
        if command.trim().is_empty() {
            return Err("Command is empty".to_string());
        }

        match self.mode {
            CommandMode::Passthrough => Ok(()),
            CommandMode::Allowlist => self.check_against_allowlist(command),
        }
    }

    /// Decides which caller-supplied environment variables may reach the child.
    ///
    /// This is an *allowlist*, not a denylist, and that is deliberate. A
    /// denylist of "dangerous" names can never be completed: `PATH` and the
    /// `LD_`/`DYLD_` loaders redirect which binary runs; `RUSTUP_TOOLCHAIN`
    /// re-points the cargo/rustc proxy shims at an arbitrary toolchain;
    /// `GIT_SSH_COMMAND`, `GIT_EXTERNAL_DIFF`, `GIT_CONFIG_COUNT`/`_KEY_*`/`_VALUE_*`
    /// and friends turn `git` into a shell; `RUSTFLAGS` injects linker
    /// arguments. Each is a general-purpose binary reading its own environment,
    /// so the only defensible boundary is to pass through a known-safe set and
    /// drop everything else.
    ///
    /// Only enforced in allowlist mode — in passthrough there is no boundary
    /// here to protect, and arbitrary env is a reasonable thing to want.
    pub fn check_env(&self, names: impl Iterator<Item = impl AsRef<str>>) -> Result<(), String> {
        match self.mode {
            CommandMode::Passthrough => return Ok(()),
            CommandMode::Allowlist => {}
        }

        for name in names {
            let name = name.as_ref();

            if !is_safe_env_name(name) {
                return Err(format!(
                    "Environment variable '{}' is refused in allowlist mode. Only a fixed set of \
                     harmless variables is passed through, because many env vars — PATH, \
                     RUSTUP_TOOLCHAIN, GIT_SSH_COMMAND, RUSTFLAGS and others — change which binary \
                     runs or what it executes, which would defeat the allowlist. Use \
                     command_mode: passthrough for this repository if arbitrary env is really wanted",
                    name
                ));
            }
        }

        Ok(())
    }

    fn check_against_allowlist(&self, command: &str) -> Result<(), String> {
        // A bare binary name only. `/usr/bin/cargo` or `./cargo` would compare
        // unequal to `cargo` and be refused anyway, but refusing them by shape
        // makes the reason obvious instead of looking like a typo.
        if command.contains('/') || command.contains('\\') {
            return Err(format!(
                "Command '{}' is refused: in allowlist mode a command must be a bare binary name, \
                 without a path",
                command
            ));
        }

        if self.allowlist.iter().any(|allowed| allowed == command) {
            return Ok(());
        }

        Err(format!(
            "Command '{}' is not in the allowlist. Allowed: {}",
            command,
            self.allowlist.join(", ")
        ))
    }
}

/// The env variables a caller may set in allowlist mode. Everything here only
/// affects output formatting or logging verbosity — none of it changes which
/// binary runs or what it executes. Matched case-insensitively.
///
/// Kept intentionally small; an operator who needs more can run the repository
/// in passthrough mode, where the boundary this protects does not apply.
const SAFE_ENV_NAMES: [&str; 11] = [
    "RUST_LOG",
    "RUST_BACKTRACE",
    "CARGO_TERM_COLOR",
    "CARGO_TERM_PROGRESS_WHEN",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "NO_COLOR",
    "FORCE_COLOR",
    "CI",
    "COLUMNS",
    "LINES",
];

fn is_safe_env_name(name: &str) -> bool {
    let name = name.trim();

    SAFE_ENV_NAMES
        .iter()
        .any(|safe| safe.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowlist_policy() -> CommandPolicy {
        CommandPolicy::new(
            CommandMode::Allowlist,
            ["cargo", "git"].iter().map(|itm| itm.to_string()).collect(),
        )
    }

    #[test]
    fn allowed_command_passes() {
        assert!(allowlist_policy().check("cargo").is_ok());
        assert!(allowlist_policy().check("git").is_ok());
    }

    #[test]
    fn command_outside_allowlist_is_refused() {
        let err = allowlist_policy().check("curl").unwrap_err();

        assert!(err.contains("not in the allowlist"), "{}", err);
    }

    #[test]
    fn allowlist_can_not_be_bypassed_with_a_path() {
        let err = allowlist_policy().check("/usr/bin/cargo").unwrap_err();

        assert!(err.contains("bare binary name"), "{}", err);

        let err = allowlist_policy().check("./cargo").unwrap_err();

        assert!(err.contains("bare binary name"), "{}", err);
    }

    #[test]
    fn empty_command_is_refused() {
        assert!(allowlist_policy().check("").is_err());
        assert!(allowlist_policy().check("   ").is_err());
    }

    #[test]
    fn passthrough_allows_anything() {
        let policy = CommandPolicy::new(CommandMode::Passthrough, Vec::new());

        assert!(policy.check("curl").is_ok());
        assert!(policy.check("/usr/bin/whatever").is_ok());
    }

    #[test]
    fn passthrough_still_refuses_an_empty_command() {
        let policy = CommandPolicy::new(CommandMode::Passthrough, Vec::new());

        assert!(policy.check("").is_err());
    }

    #[test]
    fn env_outside_the_safe_set_is_refused() {
        let policy = allowlist_policy();

        // Every one of these is a general-purpose binary reading its own
        // environment to decide what to run — the exact reason a denylist can
        // not be completed and this is an allowlist instead.
        for name in [
            "PATH",
            "path",
            "  Path  ",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "RUSTUP_TOOLCHAIN",
            "GIT_SSH_COMMAND",
            "GIT_CONFIG_COUNT",
            "GIT_EXTERNAL_DIFF",
            "RUSTFLAGS",
            "RUSTC_WRAPPER",
            "MY_PATH_TO_SOMETHING",
        ] {
            assert!(
                policy.check_env([name].iter()).is_err(),
                "{} should be refused",
                name
            );
        }
    }

    #[test]
    fn the_safe_env_set_is_allowed_regardless_of_case() {
        let policy = allowlist_policy();

        assert!(policy
            .check_env(["RUST_LOG", "CARGO_TERM_COLOR", "no_color", "Ci"].iter())
            .is_ok());
        assert!(policy.check_env(std::iter::empty::<&str>()).is_ok());
    }

    #[test]
    fn passthrough_does_not_restrict_env() {
        let policy = CommandPolicy::new(CommandMode::Passthrough, Vec::new());

        // Nothing to protect here — any binary may be started anyway.
        assert!(policy.check_env(["PATH", "GIT_SSH_COMMAND"].iter()).is_ok());
    }
}
