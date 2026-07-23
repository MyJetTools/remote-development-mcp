use dioxus_utils::DataState;
use rest_api_shared::DashboardStateResponse;

use crate::dialogs::DialogState;
use crate::states::Section;

#[derive(Default)]
pub struct AppState {
    dialog_state: DialogState,
    /// Which section the left menu has open. Survives the poll — refreshing the
    /// data must not throw the reader back to a default screen.
    pub section: Section,
    /// The whole console in one value, replaced wholesale on every poll.
    pub state: DataState<DashboardStateResponse>,
    /// Guards the refresh loop against being started twice — the component body
    /// runs on every render, the loop must not.
    pub polling_started: bool,
    /// Set when a poll fails and cleared when one succeeds. Kept beside the
    /// last good snapshot rather than replacing it, so a blip shows as "stale"
    /// instead of blanking a console someone is reading.
    pub last_error: Option<String>,
}

impl AppState {
    pub fn get_dialog_state(&self) -> &DialogState {
        &self.dialog_state
    }

    pub fn open_job_output(&mut self, repo: String, job_id: String, command_line: String) {
        self.dialog_state = DialogState::JobOutput {
            repo,
            job_id,
            command_line,
        };
    }

    pub fn close_dialog(&mut self) {
        self.dialog_state = DialogState::None;
    }

    pub fn select_section(&mut self, section: Section) {
        self.section = section;
    }

    pub fn set_snapshot(&mut self, snapshot: DashboardStateResponse) {
        self.state.set_value(snapshot);
        self.last_error = None;
    }

    pub fn set_poll_error(&mut self, err: String) {
        // Only promoted to a visible error when there is nothing to show yet.
        // Otherwise the last good snapshot stays on screen, marked stale.
        if self.state.has_value() {
            self.last_error = Some(err);
        } else {
            self.state.set_error(err);
        }
    }
}
