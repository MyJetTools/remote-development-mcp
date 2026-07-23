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

    let mut repos = Vec::with_capacity(app.projects.len());
    let mut jobs = Vec::new();

    for repo in app.projects.iter() {
        let repo_jobs = repo.jobs.list(JobStateFilter::All);

        // Looked up rather than stored on the project: an endpoint is a view
        // over projects, so this is a many-to-many the project side does not
        // know about. An empty list means the project is configured but no
        // endpoint exposes it — worth seeing on the console rather than hiding.
        let endpoints = app
            .endpoints
            .iter()
            .filter(|endpoint| {
                endpoint
                    .projects()
                    .iter()
                    .any(|exposed| exposed.name == repo.name)
            })
            .map(|endpoint| endpoint.url.to_string())
            .collect();

        repos.push(RepoModel {
            name: repo.name.clone(),
            endpoints,
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
        sessions: read_sessions(app, now),
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

/// The live sessions, joined from the two halves that know about them.
///
/// The middleware is asked which sessions exist and when each was last used —
/// it is what creates and sweeps them, and `last_access` moves on every request
/// with no event to mirror it from, so it has to be pulled rather than cached.
/// Our own registry supplies what only `initialize` carried: the ip, the country
/// the proxy reported, and the client's name.
///
/// Driving the list from the middleware also means a row can not outlive its
/// session: anything the sweeper dropped is simply not in the snapshot.
fn read_sessions(app: &Arc<AppContext>, now: DateTimeAsMicroseconds) -> Vec<SessionModel> {
    let mut result = Vec::new();

    for endpoint in app.endpoints.iter() {
        for session in endpoint.live_sessions() {
            // Absent only if the cap in the registry already dropped the row.
            // The session is real either way, so it is rendered without the
            // decoration rather than hidden.
            let known = app.sessions.get(endpoint.url, &session.id);

            let last_access = session.last_access.as_date_time();

            result.push(SessionModel {
                session_id: session.id.clone(),
                endpoint: endpoint.url.to_string(),
                ip: known
                    .as_ref()
                    .map(|known| known.ip.clone())
                    .unwrap_or_default(),
                country: known.as_ref().and_then(|known| known.country.clone()),
                country_iso3: known.as_ref().and_then(|known| known.country_iso3.clone()),
                client: known.as_ref().and_then(|known| known.client.clone()),
                protocol_version: session.version.clone(),
                connected_at: session.create.to_rfc3339(),
                age_sec: seconds_between(session.create, now),
                last_access_at: last_access.to_rfc3339(),
                idle_sec: seconds_between(last_access, now),
            });
        }
    }

    // Newest first — the order the console draws.
    result.sort_by(|left, right| {
        left.age_sec
            .partial_cmp(&right.age_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    result
}

fn seconds_between(from: DateTimeAsMicroseconds, to: DateTimeAsMicroseconds) -> f64 {
    (to.unix_microseconds - from.unix_microseconds) as f64 / 1_000_000.0
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
