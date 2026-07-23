use my_http_utils::macros::MyHttpObjectStructure;
use serde::{Deserialize, Serialize};

/// One configured repository endpoint.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct RepoModel {
    pub name: String,
    pub mcp_path: String,
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
    pub started_at: String,
    pub duration_sec: f64,
    pub remaining_sec: Option<f64>,
    pub timeout_sec: u64,
}

/// One line of the feed: a tool call, a failure, a finished job, a CI change, a
/// panic.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct HistoryEntryModel {
    pub moment: String,
    pub time_of_day: String,
    pub kind: String,
    pub repo: String,
    pub subject: String,
    pub detail: String,
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
    pub repo: String,
    pub ip: String,
    pub country: Option<String>,
    pub client: Option<String>,
    pub protocol_version: String,
    pub connected_at: String,
    pub age_sec: f64,
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
    pub uptime_sec: f64,
    pub repos: Vec<RepoModel>,
    pub sessions: Vec<SessionModel>,
    pub jobs: Vec<JobModel>,
    pub history: Vec<HistoryEntryModel>,
    pub actions: Vec<ActionRunModel>,
}
