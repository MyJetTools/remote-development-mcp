use serde::Deserialize;

/// One GitHub Actions run, as the REST API reports it.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WorkflowRun {
    pub id: u64,
    /// Workflow name, e.g. "Release App".
    #[serde(default)]
    pub name: Option<String>,
    /// The commit subject the run was started from.
    #[serde(default)]
    pub display_title: Option<String>,
    /// For a run triggered by pushing a tag this is **the tag itself**, which is
    /// what lets a release be matched to the build it started.
    #[serde(default)]
    pub head_branch: Option<String>,
    /// `queued`, `in_progress` or `completed`.
    pub status: String,
    /// Only set once `status` is `completed`: `success`, `failure`,
    /// `cancelled`, `skipped`, `timed_out`, …
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub run_number: Option<u64>,
    #[serde(default)]
    pub run_started_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub html_url: Option<String>,
}

impl WorkflowRun {
    /// True while GitHub is still working on it — the condition the poller uses
    /// to decide whether to keep asking.
    pub fn is_finished(&self) -> bool {
        self.status == "completed"
    }

    /// True when it finished and did not succeed.
    pub fn failed(&self) -> bool {
        self.is_finished()
            && !matches!(
                self.conclusion.as_deref(),
                Some("success") | Some("skipped")
            )
    }

    /// `success`, `in_progress`, … — one word for the console and the response.
    pub fn outcome(&self) -> &str {
        if self.is_finished() {
            return self.conclusion.as_deref().unwrap_or("completed");
        }

        &self.status
    }
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunsPage {
    #[serde(default)]
    pub workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WorkflowJob {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    #[serde(default)]
    pub conclusion: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowJobs {
    #[serde(default)]
    pub jobs: Vec<WorkflowJob>,
}

impl WorkflowJobs {
    /// `build / Run cargo test` — the step that actually broke.
    ///
    /// A failed run otherwise only says "failure", which means opening the web
    /// UI to learn anything. Naming the step is the difference between a status
    /// and a diagnosis.
    pub fn first_failure(&self) -> Option<String> {
        for job in self.jobs.iter() {
            if !matches!(
                job.conclusion.as_deref(),
                Some("failure") | Some("timed_out") | Some("cancelled")
            ) {
                continue;
            }

            let step = job
                .steps
                .iter()
                .find(|step| {
                    matches!(
                        step.conclusion.as_deref(),
                        Some("failure") | Some("timed_out") | Some("cancelled")
                    )
                })
                .map(|step| step.name.clone());

            return Some(match step {
                Some(step) => format!("{} / {}", job.name, step),
                None => job.name.clone(),
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: &str, conclusion: Option<&str>) -> WorkflowRun {
        WorkflowRun {
            id: 1,
            name: None,
            display_title: None,
            head_branch: None,
            status: status.to_string(),
            conclusion: conclusion.map(|value| value.to_string()),
            event: None,
            run_number: None,
            run_started_at: None,
            updated_at: None,
            html_url: None,
        }
    }

    #[test]
    fn a_run_is_finished_only_when_completed() {
        assert!(!run("queued", None).is_finished());
        assert!(!run("in_progress", None).is_finished());
        assert!(run("completed", Some("success")).is_finished());
    }

    #[test]
    fn failure_means_completed_and_not_successful() {
        assert!(run("completed", Some("failure")).failed());
        assert!(run("completed", Some("cancelled")).failed());
        assert!(run("completed", Some("timed_out")).failed());

        assert!(!run("completed", Some("success")).failed());
        // Skipped is not a failure — nothing was meant to run.
        assert!(!run("completed", Some("skipped")).failed());
        // Still running is not a failure either.
        assert!(!run("in_progress", None).failed());
    }

    #[test]
    fn the_outcome_is_the_conclusion_once_finished_and_the_status_before() {
        assert_eq!(run("in_progress", None).outcome(), "in_progress");
        assert_eq!(run("queued", None).outcome(), "queued");
        assert_eq!(run("completed", Some("failure")).outcome(), "failure");
        // A completed run with no conclusion should still say something.
        assert_eq!(run("completed", None).outcome(), "completed");
    }

    #[test]
    fn the_first_failing_step_is_named_with_its_job() {
        let jobs: WorkflowJobs = serde_json::from_str(
            r#"{"jobs":[
                {"name":"build","status":"completed","conclusion":"failure","steps":[
                    {"name":"Set up job","conclusion":"success"},
                    {"name":"Run cargo test","conclusion":"failure"}
                ]}
            ]}"#,
        )
        .unwrap();

        assert_eq!(jobs.first_failure().unwrap(), "build / Run cargo test");
    }

    #[test]
    fn a_failed_job_with_no_failed_step_still_names_the_job() {
        let jobs: WorkflowJobs = serde_json::from_str(
            r#"{"jobs":[{"name":"build","status":"completed","conclusion":"failure","steps":[]}]}"#,
        )
        .unwrap();

        assert_eq!(jobs.first_failure().unwrap(), "build");
    }

    #[test]
    fn a_successful_run_names_no_failure() {
        let jobs: WorkflowJobs = serde_json::from_str(
            r#"{"jobs":[{"name":"build","status":"completed","conclusion":"success","steps":[
                {"name":"Run cargo test","conclusion":"success"}]}]}"#,
        )
        .unwrap();

        assert!(jobs.first_failure().is_none());
    }

    #[test]
    fn a_real_api_payload_parses() {
        // Field-for-field what the REST API returned for a real run.
        let page: WorkflowRunsPage = serde_json::from_str(
            r#"{"workflow_runs":[{
                "id":29599040767,
                "name":"Release App",
                "display_title":"telegram-ingest 0.1.38: producer emits chat_id",
                "head_branch":"telegram-ingest-0.1.38",
                "status":"completed",
                "conclusion":"success",
                "event":"push",
                "run_number":25,
                "run_started_at":"2026-07-17T17:10:51Z",
                "updated_at":"2026-07-17T17:23:02Z",
                "html_url":"https://github.com/my-trading-robot/mono-repo/actions/runs/29599040767"
            }]}"#,
        )
        .unwrap();

        let run = &page.workflow_runs[0];

        assert_eq!(run.id, 29599040767);
        // The tag the release created — how a build is matched to its release.
        assert_eq!(run.head_branch.as_deref(), Some("telegram-ingest-0.1.38"));
        assert_eq!(run.outcome(), "success");
    }
}
