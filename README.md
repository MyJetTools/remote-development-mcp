# remote-development-mcp

An MCP server that lets a remote client — Claude in the cloud — build, test and
edit code in repositories on an always-on machine, over a tunnel.

Its reason to exist is the long build: a Rust workspace takes ten minutes to
compile, far longer than any single tool call should block. So commands here are
jobs. `run_command` starts one and returns immediately; `get_job_output` follows
it; `kill_job` stops it together with everything it spawned.

## Shape

One `McpMiddleware` instance is mounted per repository, each at its own URL path
from the settings:

```
MyHttpServer (bind_addr)
  ├─ AuthMiddleware              only when auth_token is set; normally absent — the proxy in front authenticates
  ├─ McpMiddleware "/my-ssh"     tools bound to ~/RustProjects/my-jet-tools/my-ssh
  ├─ McpMiddleware "/ca-api"     tools bound to ~/RustProjects/my-jet-tools/ca-api
  └─ …
```

There is no `repo` argument on any tool. A handler served at `/my-ssh` is
constructed with that repository's `RepoContext` and holds no reference to any
other root, so the isolation is structural rather than a validation step that
could be forgotten.

## Tools

**Orientation** — `repo_info`: branch, how dirty the tree is, and the workspace
crates, so a client can build one crate instead of the whole monorepo.

**Running** — `run_command`, `get_job_output`, `list_jobs`, `kill_job`.

**Version control** — `git`: runs any git command in the repository
(status, diff, log, commit, branch, checkout, …), synchronously and hardened. It
respects the command allowlist, takes no caller environment, and for a long
clone or fetch you'd use `run_command` so it becomes a job you can poll.

**Navigating** — `search` (ripgrep semantics, in-process), `list_dir`, `read_file`.

**Changing** — `write_file`, `edit_file`, `apply_patch`, `move_path`,
`delete_path`.

**Releasing** — `create_release`: creates the service's GitHub release, which
creates the tag and triggers the build. Leave `version` empty and it releases the
**next** one — it reads the tags already on GitHub, takes the highest for that
service and raises the last number. `dry_run` answers "which version would that
be?" without publishing. Tag naming follows the house guide: `{service}-{version}`
in a monorepo, the bare version in a single-service repo. Talks to the REST API
directly with `github_token`, so no `gh` CLI is needed on the machine.

**Maintenance** — `clean_cargo_targets`: reclaims disk by removing every cargo
`target` directory in the repository. Built for a monorepo of separate crates
rather than one workspace, so there is a `target` beside every `Cargo.toml`. It
only removes a `target` that has a `Cargo.toml` next to it, never follows
symlinks, and takes a `dry_run`.

## Following a long build

`run_command` returns a `job_id` plus two cursors. Each `get_job_output` call
takes the cursors from the previous one and returns everything written since,
along with fresh cursors:

```
run_command(command: "cargo", args: ["build"])
  -> { job_id: "job-000001", status: "running", next_stdout_cursor: 812, … }

get_job_output(job_id: "job-000001", stdout_cursor: 812)
  -> { status: "running", stdout: "…", next_stdout_cursor: 4096, truncated: true }
```

`truncated: true` means more output is already waiting — call again immediately
rather than sleeping. Keep going while `status` is `running`.

Cursors are byte offsets into the job's log **files**, not into an in-memory
buffer. That is what makes polling hole-free: a client that falls minutes behind
still resumes exactly where it stopped. stdout and stderr are separate files and
therefore carry separate cursors — one integer cannot address two independent
streams without losing or repeating output.

## Setup

1. `cp example-settings.yaml ~/.remote-development-mcp` and edit it. At minimum
   list your repositories.
2. `cargo run --release`
3. Expose it with a tunnel — `cloudflared tunnel --url http://127.0.0.1:8123` or
   tailscale. Do not open the port directly.
4. Add `https://<host>/<mcp_path>` as a custom connector.

Settings are read once at startup; restart to pick up changes. A bad repository
root or a duplicated `mcp_path` stops the server coming up
rather than turning into an endpoint that fails every call.

`git` needs to be installed — `apply_patch`, `list_dir`'s gitignore filtering,
`repo_info` and the `git` tool shell out to it. `search` does **not** need
ripgrep: it uses ripgrep's libraries in-process. `create_release` needs no `gh`
CLI either, only `github_token` in the settings.

## What is actually guaranteed

Be clear-eyed about the threat model: **this server does not authenticate**. It
is meant to run behind a reverse proxy that terminates authentication, and it
trusts whatever reaches its port — so bind it to loopback and let nothing but
that proxy talk to it. (`auth_token` exists as a fallback if you ever expose it
directly.) On top of that, an allowlisted `cargo build` runs `build.rs` —
arbitrary code — by design. So this is not a sandbox that contains a hostile client. What it does do
is confine the **file tools**, keep the **command surface** small and honest, and
record everything.

**File-tool path confinement holds.** Every path given to `read_file`,
`write_file`, `edit_file`, `list_dir`, `search`, `move_path` or `delete_path`,
and every command's `cwd`, is resolved against the repository root and refused if
it lands outside. The deepest existing ancestor is canonicalized, which resolves
symlinks and `..`, so neither a symlink out of the tree nor `../../etc/passwd`
gets through, and a sibling directory sharing a name prefix is not mistaken for
the root. Paths that do not exist yet are supported, because `write_file` needs
them. `move_path` and `delete_path` act on a symlink as the link, not its target.

**`run_command` arguments are not confined.** The path confinement covers the
file tools and `cwd`, not the strings handed to a process — `git show HEAD:foo`
reads a tracked file regardless. That is deliberate and is why the default
allowlist (`cargo`, `rustc`, `git`, `rg`) contains no general file-I/O binaries:
`cat`, `ls`, `mv`, `mkdir` would read and write outside the root through their
own arguments, and the confined file tools cover them instead. Treat
`run_command` as running trusted build tooling.

**The command policy resists redirection.** In allowlist mode the binary must be
a bare allowlisted name, and the caller-supplied environment is filtered to a
small safe set. This is an allowlist, not a denylist, on purpose: `PATH`,
`RUSTUP_TOOLCHAIN`, `GIT_SSH_COMMAND`, `RUSTFLAGS` and many others each redirect
what actually runs, and that set can not be enumerated. The server's own `git`
calls also run with hooks, `core.fsmonitor` and the `ext::` transport disabled.

**`.git` is write-protected.** No tool will write inside a `.git` directory, so a
client cannot plant a `core.fsmonitor` or a hook that the server's own `git`
would then execute — a code-execution path that would otherwise bypass the
allowlist and the audit log entirely. Reads of `.git` are still allowed.

**Auditing is available, off by default.** Set `audit_log_path` to turn it on;
every command start, finish and refusal is then appended as a JSON line —
including refusals, since a run of those is what an attempt to get around the
allowlist looks like. With no path set, nothing is written.

**`delete_path` is off** unless a repository sets `allow_delete: true`. It is the
one tool here that git cannot undo.

## Development

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The tests cover the invariants worth being sure about: path confinement against
`..`, symlinks, dangling symlinks and prefix-sharing siblings; allowlist
refusals; the full job lifecycle — a long command polled through to its exit
code with the output reassembled from cursors and compared byte for byte —
plus timeout and kill producing `timed_out` and `killed` rather than a bare
`exited`.
