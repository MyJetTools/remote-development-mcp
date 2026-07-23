use std::sync::Arc;

use my_http_server::controllers::ControllersMiddleware;

use crate::app::AppContext;

/// The REST surface the browser console reads. Everything it returns is held in
/// memory by the running server — there is no store behind it.
pub fn build_controllers(app: &Arc<AppContext>) -> ControllersMiddleware {
    let mut controllers = ControllersMiddleware::new(None, None);

    controllers.register_get_action(Arc::new(
        super::controllers::dashboard::GetStateAction::new(app.clone()),
    ));

    controllers.register_get_action(Arc::new(super::controllers::jobs::GetOutputAction::new(
        app.clone(),
    )));

    controllers
}
