use std::sync::Arc;

use app::AppContext;

use crate::settings::SettingsModel;

mod app;
mod audit;
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

    // Everything the settings describe is validated while the context is built,
    // so a misconfiguration stops the server here rather than turning into an
    // endpoint which fails every call.
    let app = match AppContext::new(settings).await {
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

    crate::http::start(&app).await;

    app.app_states.wait_until_shutdown().await;
}
