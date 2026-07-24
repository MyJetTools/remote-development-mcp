use std::time::Duration;

use dioxus::prelude::*;
use dioxus_utils::RenderState;
use rest_api_shared::DashboardStateResponse;

use crate::states::AppState;

/// How often the console refreshes. The server answers from memory, so this
/// costs a map read — not a query.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Everything that has to outlive a tab switch: the state, the poll loop, the
/// palette, the top bar and the dialog overlay.
///
/// A router layout rather than a component per tab, and that is the whole point
/// — a layout is mounted once and only its `Outlet` swaps underneath, so moving
/// between tabs keeps the snapshot on screen. Were each tab to own this, every
/// click would rebuild `AppState`, restart the loop and flash "connecting…".
#[component]
pub fn Shell() -> Element {
    let app_state = use_context_provider(|| Signal::new(AppState::default()));
    let app_state_ra = app_state.read();

    if !app_state_ra.polling_started {
        start_polling(app_state);
    }

    // The one place the palette is chosen. Everything below reads its colours
    // from variables, so the whole console changes with this class and nothing
    // else has to know a theme exists. No class at all means the stylesheet's
    // own `prefers-color-scheme` default is left to decide.
    let theme_class = app_state_ra.theme.class();
    let stale = app_state_ra.last_error.clone();

    let snapshot = match app_state_ra.state.as_ref() {
        RenderState::None | RenderState::Loading => Err(rsx! {
            div { class: "loading-screen", "connecting…" }
        }),
        RenderState::Error(err) => Err(rsx! {
            div { class: "error-screen", "{err}" }
        }),
        RenderState::Loaded(state) => Ok(state.clone()),
    };

    // Dropped before the tabs render: they read this same signal, and holding a
    // guard across them is asking for a deadlock the first time one writes.
    drop(app_state_ra);

    let body = match snapshot {
        Err(note) => note,
        Ok(state) => rsx! {
            crate::components::TopBar { state, stale }
            div { class: "content",
                Outlet::<crate::AppRoute> {}
            }
        },
    };

    rsx! {
        // Wraps the dialog too, not just the panel: the dialog is a fixed
        // overlay outside the panel's box, and left outside this it would keep
        // the system palette while everything behind it changed.
        div { class: "app-root {theme_class}",
            div { id: "main-panel", {body} }
            crate::dialogs::RenderDialog {}
        }
    }
}

/// The projects this server exposes and the urls they are reached at. Scrolls as
/// one block.
#[component]
pub fn ProjectsTab() -> Element {
    let app_state = consume_context::<Signal<AppState>>();
    let app_state_ra = app_state.read();

    let Some(state) = snapshot(&app_state_ra) else {
        return rsx! {};
    };

    drop(app_state_ra);

    rsx! {
        div { class: "section-scroll",
            crate::components::ReposPanel { repos: state.repos }
        }
    }
}

/// Browsing one project's tree and looking at what is in it.
///
/// Two columns filling the height, each scrolling on its own — the tree must not
/// scroll the file away, and the file must not scroll the tree.
#[component]
pub fn FilesTab(selected: String) -> Element {
    let app_state = consume_context::<Signal<AppState>>();
    let app_state_ra = app_state.read();

    let Some(state) = snapshot(&app_state_ra) else {
        return rsx! {};
    };

    drop(app_state_ra);

    rsx! {
        crate::views::files::RenderFiles { repos: state.repos, selected }
    }
}

/// The live MCP sessions.
#[component]
pub fn SessionsTab() -> Element {
    let app_state = consume_context::<Signal<AppState>>();
    let app_state_ra = app_state.read();

    let Some(state) = snapshot(&app_state_ra) else {
        return rsx! {};
    };

    let tz = app_state_ra.time_zone();

    drop(app_state_ra);

    rsx! {
        div { class: "section-scroll",
            crate::components::SessionsPanel { sessions: state.sessions, tz }
        }
    }
}

/// What the server is doing: running commands, the call feed, CI builds.
///
/// Jobs and CI keep their natural height at the top; History takes the rest of
/// the column and scrolls inside it.
#[component]
pub fn TasksTab() -> Element {
    let app_state = consume_context::<Signal<AppState>>();
    let app_state_ra = app_state.read();

    let Some(state) = snapshot(&app_state_ra) else {
        return rsx! {};
    };

    let tz = app_state_ra.time_zone();

    drop(app_state_ra);

    rsx! {
        div { class: "section-fill",
            crate::components::JobsPanel { jobs: state.jobs }
            crate::components::ActionsPanel { actions: state.actions }
            crate::components::HistoryPanel { history: state.history, tz }
        }
    }
}

/// The snapshot a tab draws from.
///
/// `Shell` has already gated on it, so in practice there is always one by the
/// time a tab renders; `None` draws nothing rather than a second "connecting…"
/// under the one the shell is showing.
fn snapshot(app_state: &AppState) -> Option<DashboardStateResponse> {
    match app_state.state.as_ref() {
        RenderState::Loaded(state) => Some(state.clone()),
        _ => None,
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
