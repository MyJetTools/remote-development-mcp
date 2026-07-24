use dioxus::prelude::*;

mod api;
mod components;
mod dialogs;
mod models;
mod states;
mod time;
mod views;
mod web;

use views::dashboard::{FilesTab, ProjectsTab, SessionsTab, Shell, TasksTab};

/// One url per tab, so a tab can be linked to, bookmarked and reloaded into.
///
/// `Tasks` sits on the root rather than under `/tasks`: it is what the console
/// is for — what the server is doing right now — so a bare link to the machine
/// should land on it directly instead of on a redirect.
///
/// Every route hangs off `Shell`, and that is what makes tab switching cheap: a
/// layout is mounted once and only its `Outlet` swaps underneath, so moving
/// between tabs keeps the snapshot and does not restart the poll loop.
///
/// Deep links work because the server serves `index.html` for unknown paths
/// (`set_not_found_file` in its static middleware) — `/files` reaches this
/// router in the browser rather than 404ing on the way in.
#[derive(Routable, Clone, PartialEq)]
pub enum AppRoute {
    #[layout(Shell)]
    #[route("/")]
    TasksTab {},
    #[route("/projects")]
    ProjectsTab {},
    /// The selected file rides in the url so that a file can be linked to, and
    /// so a reload comes back to it — the tree opens every folder above it on
    /// the way. Empty means nothing is selected, which is what a bare `/files`
    /// parses to.
    #[route("/files?:selected")]
    FilesTab { selected: String },
    #[route("/sessions")]
    SessionsTab {},
}

fn main() {
    dioxus::LaunchBuilder::new().launch(|| {
        rsx! {
            document::Link { rel: "icon", href: asset!("/public/favicon.ico") }
            document::Meta {
                name: "viewport",
                content: "width=device-width, initial-scale=1.0",
            }
            Router::<AppRoute> {}
        }
    });
}
