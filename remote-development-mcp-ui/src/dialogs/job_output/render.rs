use std::time::Duration;

use dioxus::prelude::*;

use super::JobOutputState;

/// How often a running job's output is asked for. Fast enough to read a build
/// as it happens, and each call only carries what is new.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[component]
pub fn JobOutputDialog(repo: String, job_id: String, command_line: String) -> Element {
    let cs = use_signal(JobOutputState::default);
    let cs_ra = cs.read();

    if !cs_ra.polling_started {
        start_polling(cs, repo.clone(), job_id.clone());
    }

    let title = rsx! {
        span { "{command_line}" }
        span { class: "dim", " · {repo} · {job_id}" }
    };

    let content = if !cs_ra.loaded_once {
        rsx! {
            div { class: "loading-screen", "reading output…" }
        }
    } else {
        let status = cs_ra.status.as_str();
        let exit = match cs_ra.exit_code {
            Some(code) => format!(" (exit {})", code),
            None => String::new(),
        };

        rsx! {
            div { class: "dim", "status: {status}{exit}" }

            if let Some(err) = cs_ra.error.as_ref() {
                div { class: "error-screen", "{err}" }
            }

            if !cs_ra.stdout.is_empty() {
                div { class: "stream-label", "stdout" }
                pre { class: "stream", "{cs_ra.stdout}" }
            }

            if !cs_ra.stderr.is_empty() {
                div { class: "stream-label", "stderr" }
                pre { class: "stream stderr", "{cs_ra.stderr}" }
            }

            if cs_ra.stdout.is_empty() && cs_ra.stderr.is_empty() {
                div { class: "dim", "no output yet" }
            }
        }
    };

    crate::dialogs::dialog_template(title, content)
}

/// Reads from where the last read stopped until the job ends.
fn start_polling(mut cs: Signal<JobOutputState>, repo: String, job_id: String) {
    spawn(async move {
        {
            let mut w = cs.write();

            if w.polling_started {
                return;
            }

            w.polling_started = true;
        }

        loop {
            let (stdout_cursor, stderr_cursor) = {
                let ra = cs.read();
                (ra.stdout_cursor, ra.stderr_cursor)
            };

            let result = crate::api::jobs::get_output(
                repo.clone(),
                job_id.clone(),
                stdout_cursor,
                stderr_cursor,
            )
            .await;

            match result {
                Ok(chunk) => cs.write().append(chunk),
                Err(err) => cs.write().set_error(err.to_string()),
            }

            if cs.read().is_finished() {
                return;
            }

            dioxus_utils::js::sleep(POLL_INTERVAL).await;
        }
    });
}
