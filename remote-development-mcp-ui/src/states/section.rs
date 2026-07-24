/// Which section the left menu has open. Each one fills the whole content area
/// on its own, so the console shows one thing at a time rather than every panel
/// stacked.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    /// The projects this server exposes and the urls they are reached at.
    Projects,
    /// Browsing one project's tree and looking at what is in it.
    Files,
    /// The live MCP sessions.
    Sessions,
    /// What the server is doing: running commands, the call feed, CI builds.
    #[default]
    Tasks,
}

impl Section {
    /// The order they appear in the menu.
    pub const ALL: [Section; 4] = [
        Section::Projects,
        Section::Files,
        Section::Sessions,
        Section::Tasks,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Section::Projects => "Projects",
            Section::Files => "Files",
            Section::Sessions => "Sessions",
            Section::Tasks => "Tasks",
        }
    }
}
