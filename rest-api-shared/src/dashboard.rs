use my_http_utils::macros::MyHttpObjectStructure;
use serde::{Deserialize, Serialize};

/// One configured project.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct RepoModel {
    pub name: String,
    /// Urls this project is reachable through. Several, because an endpoint is
    /// a view over projects rather than a property of one — and empty when the
    /// project is configured but no endpoint exposes it, which is worth seeing.
    pub endpoints: Vec<String>,
    pub root: String,
    pub description: Option<String>,
    pub running_jobs: usize,
}

/// A command the server started, running or finished.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct JobModel {
    pub repo: String,
    pub job_id: String,
    pub command_line: String,
    pub cwd: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
    /// When it started, Unix microseconds. A number the client rebuilds with
    /// `DateTimeAsMicroseconds::new`, not a preformatted string.
    pub started_at: i64,
    pub duration_sec: f64,
    pub remaining_sec: Option<f64>,
    pub timeout_sec: u64,
}

/// One line of the feed: a tool call, a failure, a finished job, a CI change, a
/// panic.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct HistoryEntryModel {
    /// When it happened, Unix microseconds — rebuilt client-side with
    /// `DateTimeAsMicroseconds::new` and rendered in the viewer's zone.
    pub moment: i64,
    pub kind: String,
    pub repo: String,
    pub subject: String,
    pub detail: String,
    /// How long it took — a synchronous tool call's own time, or a finished
    /// job's run time. Absent for entries with no duration to report, such as a
    /// panic or a CI state change.
    pub duration_sec: Option<f64>,
}

/// A GitHub Actions run the server is following.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct ActionRunModel {
    pub repo: String,
    pub run_id: u64,
    pub workflow: String,
    pub tag: Option<String>,
    pub outcome: String,
    pub finished: bool,
    pub failed_step: Option<String>,
    pub url: Option<String>,
    pub elapsed_sec: f64,
}

/// One live MCP session.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct SessionModel {
    pub session_id: String,
    /// The url the client connected to. Sessions belong to an endpoint, not to
    /// a project — one session can reach every project that endpoint exposes.
    pub endpoint: String,
    pub ip: String,
    /// Exactly what the proxy reported, normally iso2. Kept as the label, since
    /// it is what is true rather than what was recognised.
    pub country: Option<String>,
    /// The same country as iso3, which is how the flag assets are named. Absent
    /// when the reported code parsed as no country at all — the row still
    /// renders, just without a flag.
    pub country_iso3: Option<String>,
    pub client: Option<String>,
    pub protocol_version: String,
    /// Unix microseconds; the client rebuilds it with `DateTimeAsMicroseconds::new`.
    pub connected_at: i64,
    pub age_sec: f64,
    /// When a request last arrived on this session — any request, `ping`
    /// included. Read live from the middleware on every poll, because it is the
    /// same value its idle sweeper decides by.
    pub last_access_at: i64,
    /// Seconds since that request. What the sweeper compares to the idle
    /// timeout, so a row climbing towards it is a session about to be dropped.
    pub idle_sec: f64,
}

/// Everything the console shows, in one snapshot.
///
/// One endpoint rather than four because the page polls: a single round trip
/// gives a consistent picture, where four could show a job as running in one
/// pane and finished in another.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct DashboardStateResponse {
    pub app_name: String,
    pub version: String,
    pub bind_addr: String,
    /// The instant this snapshot was taken, Unix microseconds (UTC). Paired with
    /// the browser's own clock at receipt, it is what lets the console derive
    /// the viewer's timezone and render every other instant in local wall-clock
    /// time. A raw number the client rebuilds with `DateTimeAsMicroseconds::new`,
    /// never a preformatted string — the server has no idea where the viewer is.
    pub server_time: i64,
    pub uptime_sec: f64,
    pub repos: Vec<RepoModel>,
    pub sessions: Vec<SessionModel>,
    pub jobs: Vec<JobModel>,
    pub history: Vec<HistoryEntryModel>,
    pub actions: Vec<ActionRunModel>,
}
