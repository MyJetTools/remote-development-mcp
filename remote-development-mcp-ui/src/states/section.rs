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

    /// The url this section lives at, for the menu to link to.
    pub fn route(&self) -> crate::AppRoute {
        match self {
            Section::Projects => crate::AppRoute::ProjectsTab {},
            Section::Files => crate::AppRoute::FilesTab {},
            Section::Sessions => crate::AppRoute::SessionsTab {},
            Section::Tasks => crate::AppRoute::TasksTab {},
        }
    }

    /// Which section a url is showing.
    ///
    /// The route is the single source of truth for that now — it used to be a
    /// field on `AppState`, and keeping both would have meant the menu could
    /// highlight one tab while the content showed another.
    pub fn from_route(route: &crate::AppRoute) -> Self {
        match route {
            crate::AppRoute::ProjectsTab {} => Section::Projects,
            crate::AppRoute::FilesTab {} => Section::Files,
            crate::AppRoute::SessionsTab {} => Section::Sessions,
            crate::AppRoute::TasksTab {} => Section::Tasks,
        }
    }
}
