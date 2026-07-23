use std::sync::Arc;

use app::AppContext;

use crate::{activity::ActivityLog, settings::SettingsModel};

mod actions;
mod activity;
mod app;
mod audit;
mod github;
mod http;
mod jobs;
mod mcp;
mod repo;
mod scripts;
mod sessions;
mod settings;

const SETTINGS_FILE: &str = "~/.remote-development-mcp";

#[tokio::main]
async fn main() {
    my_logger::LOGGER
        .populate_app_and_version(app::APP_NAME, app::APP_VERSION)
        .await;

    // Everything that happens is kept here in memory and read over the REST API
    // by the browser console. Nothing of it goes to the terminal — see
    // `ActivityLog`.
    let activity = Arc::new(ActivityLog::new());

    // First, so every panic from here on is covered: it goes into the feed the
    // browser renders *and* to the terminal, which carries nothing else and so
    // cannot bury it.
    crate::activity::install(activity.clone());

    let settings = match SettingsModel::load(SETTINGS_FILE).await {
        Ok(settings) => settings,
        Err(err) => panic!("Can not read the settings from {}. {}", SETTINGS_FILE, err),
    };

    // Everything the settings describe is validated while the context is built,
    // so a misconfiguration stops the server here rather than turning into an
    // endpoint which fails every call.
    let app = match AppContext::new(settings, activity).await {
        Ok(app) => app,
        Err(err) => panic!("Can not start: {}", err),
    };

    let app = Arc::new(app);

    for project in app.projects.iter() {
        println!("Project '{}' -> {}", project.name, project.root().display());
    }

    // Printed after the projects, and listing them by id: a project missing from
    // every line here is configured but reachable through nothing, which is easy
    // to do and invisible otherwise.
    for endpoint in app.endpoints.iter() {
        println!(
            "Serving {} -> {}",
            endpoint.url,
            endpoint
                .projects()
                .iter()
                .map(|project| project.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    println!("Listening on {}", app.bind_addr);
    println!("Console: http://{}/", app.bind_addr);

    // Keeps the followed builds fresh whether or not anyone is asking, which is
    // what lets the console show one finishing on its own.
    tokio::spawn(crate::actions::run_poller(
        app.watched_runs.clone(),
        app.activity.clone(),
        app.projects
            .iter()
            .find_map(|project| project.github_token.clone()),
    ));

    crate::http::start(&app).await;

    app.app_states.wait_until_shutdown().await;
}
