use dioxus_utils::DataState;
use rest_api_shared::DashboardStateResponse;
use rust_extensions::date_time::TimeZone;

use crate::dialogs::DialogState;
use crate::states::Theme;

#[derive(Default)]
pub struct AppState {
    dialog_state: DialogState,
    /// Which palette to render in. Starts at "whatever the machine says" and
    /// only stops following it when the reader picks one.
    pub theme: Theme,
    /// The viewer's timezone, derived from each snapshot's `server_time` against
    /// the browser clock. `None` until the first snapshot lands; every instant
    /// is rendered through it. Recomputed each poll so the console follows the
    /// browser across a DST change without a reload.
    pub time_zone: Option<TimeZone>,
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

    pub fn cycle_theme(&mut self) {
        self.theme = self.theme.next();
    }

    pub fn set_snapshot(&mut self, snapshot: DashboardStateResponse) {
        // Derived here, at receipt, so the browser clock it is paired with is
        // read as close as possible to the server clock in the payload.
        self.time_zone = Some(crate::time::timezone_from_snapshot(snapshot.server_time));
        self.state.set_value(snapshot);
        self.last_error = None;
    }

    /// The viewer's timezone, or UTC until the first snapshot has derived it.
    pub fn time_zone(&self) -> TimeZone {
        self.time_zone.unwrap_or_else(TimeZone::utc)
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
