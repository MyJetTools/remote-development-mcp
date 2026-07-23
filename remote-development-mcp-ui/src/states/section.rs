/// Which section the left menu has open. Each one fills the whole content area
/// on its own, so the console shows one thing at a time rather than every panel
/// stacked.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    /// The projects this server exposes and the urls they are reached at.
    Projects,
    /// The live MCP sessions.
    Sessions,
    /// What the server is doing: running commands, the call feed, CI builds.
    #[default]
    Tasks,
}

impl Section {
    /// The order they appear in the menu.
    pub const ALL: [Section; 3] = [Section::Projects, Section::Sessions, Section::Tasks];

    pub fn label(&self) -> &'static str {
        match self {
            Section::Projects => "Projects",
            Section::Sessions => "Sessions",
            Section::Tasks => "Tasks",
        }
    }
}
