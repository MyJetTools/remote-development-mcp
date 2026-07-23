use std::time::Duration;

use dioxus::prelude::*;
use dioxus_utils::RenderState;

use crate::states::AppState;

/// How often the console refreshes. The server answers from memory, so this
/// costs a map read — not a query.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[component]
pub fn RenderDashboard() -> Element {
    let app_state = consume_context::<Signal<AppState>>();
    let app_state_ra = app_state.read();

    if !app_state_ra.polling_started {
        start_polling(app_state);
    }

    let state = match app_state_ra.state.as_ref() {
        RenderState::None | RenderState::Loading => {
            return rsx! {
                div { class: "loading-screen", "connecting…" }
            }
        }
        RenderState::Error(err) => {
            return rsx! {
                div { class: "error-screen", "{err}" }
            }
        }
        RenderState::Loaded(state) => state.clone(),
    };

    let stale = app_state_ra.last_error.clone();

    rsx! {
        crate::components::Header { state: state.clone(), stale }
        div { class: "top-region",
            crate::components::ReposPanel { repos: state.repos.clone() }
            crate::components::SessionsPanel { sessions: state.sessions.clone() }
            crate::components::JobsPanel { jobs: state.jobs.clone() }
            crate::components::ActionsPanel { actions: state.actions.clone() }
        }
        crate::components::HistoryPanel { history: state.history }
    }
}

/// Refreshes the whole snapshot forever.
///
/// A failed poll does not end the loop: the Mac mini this runs on is reachable
/// over a tunnel, and a console that gave up the first time the tunnel blinked
/// would need reloading by hand.
fn start_polling(mut app_state: Signal<AppState>) {
    spawn(async move {
        {
            let mut w = app_state.write();

            if w.polling_started {
                return;
            }

            w.polling_started = true;
            w.state.set_loading();
        }

        loop {
            match crate::api::dashboard::get_state().await {
                Ok(state) => app_state.write().set_snapshot(state),
                Err(err) => app_state.write().set_poll_error(err.to_string()),
            }

            dioxus_utils::js::sleep(POLL_INTERVAL).await;
        }
    });
}
