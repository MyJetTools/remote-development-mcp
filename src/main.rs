use std::sync::Arc;

use app::AppContext;

use crate::{activity::ActivityLog, settings::SettingsModel};

mod activity;
mod app;
mod audit;
mod http;
mod jobs;
mod mcp;
mod repo;
mod scripts;
mod settings;
mod tui;

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

    // With a terminal, activity is drawn by the console; without one — under
    // launchd, a pipe, a redirect — it is printed as plain lines instead, since
    // taking over a screen that is not there would only emit escape codes.
    let with_console = crate::tui::stdout_is_a_terminal();
    let activity = Arc::new(ActivityLog::new(!with_console));

    // Everything the settings describe is validated while the context is built,
    // so a misconfiguration stops the server here rather than turning into an
    // endpoint which fails every call.
    let app = match AppContext::new(settings, activity).await {
        Ok(app) => app,
        Err(err) => panic!("Can not start: {}", err),
    };

    let app = Arc::new(app);

    if !with_console {
        for repo in app.repos.iter() {
            println!(
                "Serving '{}' at {} -> {}",
                repo.name,
                repo.mcp_path,
                repo.root().display()
            );
        }

        println!("Listening on {}", app.bind_addr);
    }

    crate::http::start(&app).await;

    if with_console {
        tokio::spawn(crate::tui::run(app.clone()));
    }

    app.app_states.wait_until_shutdown().await;
}
