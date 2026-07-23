pub mod dashboard;
pub mod jobs;

use flurl::{FlUrlError, FlUrlResponse};
use serde::de::DeserializeOwned;

use crate::models::RequestError;

fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

async fn read_error_body(response: &mut FlUrlResponse) -> RequestError {
    let message = response
        .get_body_as_str()
        .await
        .map(|body| body.to_string())
        .unwrap_or_else(|err| err.to_string());

    RequestError { message }
}

/// 2xx deserializes into `T`; anything else carries the server's own message
/// back to the caller, so a failure reads as what the server said rather than
/// "request failed".
pub async fn handle_http_response<T: DeserializeOwned>(
    response: Result<FlUrlResponse, FlUrlError>,
) -> Result<T, RequestError> {
    let mut response = response?;

    if is_success(response.get_status_code()) {
        return Ok(response.get_json().await?);
    }

    Err(read_error_body(&mut response).await)
}
