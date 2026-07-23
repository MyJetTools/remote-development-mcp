use my_http_utils::macros::{MyHttpInput, MyHttpObjectStructure};
use serde::{Deserialize, Serialize};

#[derive(MyHttpInput)]
pub struct JobOutputRequestModel {
    #[http_query(
        name = "repo",
        description = "Repository endpoint name the job belongs to"
    )]
    pub repo: String,

    #[http_query(name = "jobId", description = "Job id, as run_command returned it")]
    pub job_id: String,

    #[http_query(
        name = "stdoutCursor",
        description = "Byte offset to resume stdout from",
        default = 0
    )]
    pub stdout_cursor: u64,

    #[http_query(
        name = "stderrCursor",
        description = "Byte offset to resume stderr from",
        default = 0
    )]
    pub stderr_cursor: u64,
}

#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct JobOutputResponse {
    pub job_id: String,
    pub command_line: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub next_stdout_cursor: u64,
    pub next_stderr_cursor: u64,
    pub has_more: bool,
}
