use dioxus::prelude::*;

use crate::states::AppState;

use super::DialogState;

#[component]
pub fn RenderDialog() -> Element {
    let app_state = consume_context::<Signal<AppState>>();
    let app_state_ra = app_state.read();

    if app_state_ra.get_dialog_state().is_hidden() {
        return rsx! {};
    }

    match app_state_ra.get_dialog_state() {
        DialogState::None => rsx! {},
        DialogState::JobOutput {
            repo,
            job_id,
            command_line,
        } => {
            let repo = repo.clone();
            let job_id = job_id.clone();
            let command_line = command_line.clone();
            drop(app_state_ra);

            rsx! {
                super::job_output::JobOutputDialog { repo, job_id, command_line }
            }
        }
    }
}

/// The wrapper every dialog uses. The close button lives here, so no dialog has
/// to render one of its own.
pub fn dialog_template(title: Element, content: Element) -> Element {
    let mut app_state = consume_context::<Signal<AppState>>();

    rsx! {
        div { class: "dialog-backdrop",
            div { class: "dialog",
                div { class: "dialog-head",
                    div { class: "title", {title} }
                    div { class: "spacer" }
                    button {
                        class: "dialog-close",
                        onclick: move |_| app_state.write().close_dialog(),
                        "✕"
                    }
                }
                div { class: "dialog-body", {content} }
            }
        }
    }
}
