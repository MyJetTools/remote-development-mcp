use std::sync::Arc;

use rest_api_shared::{
    ActionRunModel, DashboardStateResponse, HistoryEntryModel, JobModel, RepoModel, SessionModel,
};
use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::{
    activity::ActivityEvent,
    app::{AppContext, APP_NAME, APP_VERSION},
    jobs::{Job, JobStateFilter},
};

/// Reads everything the console shows, in one pass.
///
/// Every source is already an in-memory structure guarded by a `parking_lot`
/// lock, so this touches no disk and can be answered on every poll.
pub fn read_dashboard_state(app: &Arc<AppContext>) -> DashboardStateResponse {
    let now = DateTimeAsMicroseconds::now();

    let mut repos = Vec::with_capacity(app.repos.len());
    let mut jobs = Vec::new();

    for repo in app.repos.iter() {
        let repo_jobs = repo.jobs.list(JobStateFilter::All);

        repos.push(RepoModel {
            name: repo.name.clone(),
            mcp_path: repo.mcp_path.to_string(),
            root: repo.root().display().to_string(),
            description: repo.description.clone(),
            running_jobs: repo_jobs
                .iter()
                .filter(|job| job.status.is_running())
                .count(),
        });

        for job in repo_jobs {
            jobs.push(to_job_model(&repo.name, job, now));
        }
    }

    // Newest first, and a running job outranks a finished one however recent —
    // what is happening now belongs at the top.
    jobs.sort_by(|left, right| {
        let running = right
            .remaining_sec
            .is_some()
            .cmp(&left.remaining_sec.is_some());

        running.then_with(|| right.started_at.cmp(&left.started_at))
    });

    DashboardStateResponse {
        app_name: APP_NAME.to_string(),
        version: APP_VERSION.to_string(),
        bind_addr: app.bind_addr.clone(),
        uptime_sec: (now.unix_microseconds - app.started_at.unix_microseconds) as f64 / 1_000_000.0,
        repos,
        sessions: app
            .sessions
            .all()
            .into_iter()
            .map(|session| SessionModel {
                session_id: session.session_id.clone(),
                repo: session.repo.clone(),
                ip: session.ip.clone(),
                country: session.country.clone(),
                client: session.client.clone(),
                protocol_version: session.protocol_version.clone(),
                connected_at: session.connected_at.to_rfc3339(),
                age_sec: session.age_sec(now),
            })
            .collect(),
        jobs,
        // The whole ring: it is already bounded, so a second limit here could
        // only drift from it.
        history: app
            .activity
            .recent(usize::MAX)
            .into_iter()
            .map(to_history_model)
            .collect(),
        actions: app
            .watched_runs
            .all()
            .into_iter()
            .map(|run| ActionRunModel {
                repo: run.repo.clone(),
                run_id: run.run_id,
                workflow: run.workflow.clone(),
                tag: run.tag.clone(),
                outcome: run.outcome.clone(),
                finished: run.finished,
                failed_step: run.failed_step.clone(),
                url: run.url.clone(),
                elapsed_sec: run.elapsed_sec(now),
            })
            .collect(),
    }
}

fn to_job_model(repo: &str, job: Job, now: DateTimeAsMicroseconds) -> JobModel {
    JobModel {
        repo: repo.to_string(),
        job_id: job.id.clone(),
        command_line: job.command_line.clone(),
        cwd: job.cwd.clone(),
        status: job.status.as_str().to_string(),
        exit_code: job.exit_code,
        pid: job.pid,
        started_at: job.started_at.to_rfc3339(),
        duration_sec: job.duration_sec(now),
        remaining_sec: job.remaining_sec(now),
        timeout_sec: job.timeout_sec,
    }
}

fn to_history_model(event: ActivityEvent) -> HistoryEntryModel {
    HistoryEntryModel {
        moment: event.moment.to_rfc3339(),
        time_of_day: event.time_of_day(),
        kind: event.kind.as_str().to_string(),
        repo: event.repo,
        subject: event.subject,
        detail: event.detail,
    }
}
