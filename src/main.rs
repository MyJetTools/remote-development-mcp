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
mod settings;

const SETTINGS_FILE: &str = "~/.remote-development-mcp";

#[tokio::main]
async fn main() {
    my_logger::LOGGER
        .populate_app_and_version(app::APP_NAME, app::APP_VERSION)
        .await;

    let settings = match SettingsModel::load(SETTINGS_FILE).await {
        Ok(settings) => settings,
        Err(err) => panic!("Can not read the settings from {}. {}", SETTINGS_FILE, err),
    };

    // Every event is echoed to stdout as it happens and kept in memory for the
    // browser console to read. Nothing takes over the terminal, so the ordinary
    // things a terminal is good at — scrollback, piping to a file, launchd
    // capturing the output — all keep working.
    let activity = Arc::new(ActivityLog::new(true));

    // Installed before anything can panic, so a panic lands in the feed the
    // browser renders as well as on the terminal.
    crate::activity::install(activity.clone());

    // Everything the settings describe is validated while the context is built,
    // so a misconfiguration stops the server here rather than turning into an
    // endpoint which fails every call.
    let app = match AppContext::new(settings, activity).await {
        Ok(app) => app,
        Err(err) => panic!("Can not start: {}", err),
    };

    let app = Arc::new(app);

    for repo in app.repos.iter() {
        println!(
            "Serving '{}' at {} -> {}",
            repo.name,
            repo.mcp_path,
            repo.root().display()
        );
    }

    println!("Listening on {}", app.bind_addr);
    println!("Console: http://{}/", app.bind_addr);

    // Keeps the followed builds fresh whether or not anyone is asking, which is
    // what lets the console show one finishing on its own.
    tokio::spawn(crate::actions::run_poller(
        app.watched_runs.clone(),
        app.activity.clone(),
        app.repos.iter().find_map(|repo| repo.github_token.clone()),
    ));

    crate::http::start(&app).await;

    app.app_states.wait_until_shutdown().await;
}
