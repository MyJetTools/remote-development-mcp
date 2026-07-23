#[derive(Default)]
pub enum DialogState {
    #[default]
    None,
    JobOutput {
        repo: String,
        job_id: String,
        command_line: String,
    },
}

impl DialogState {
    pub fn is_hidden(&self) -> bool {
        matches!(self, Self::None)
    }
}
