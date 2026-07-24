use flurl::{FlUrl, HttpVerb};
use rest_api_shared::{
    FileContentResponse, FileRequestModel, ListFolderRequestModel, ListFolderResponse,
};

use crate::models::RequestError;

pub async fn list_folder(
    repo: String,
    path: Option<String>,
) -> Result<ListFolderResponse, RequestError> {
    let request = ListFolderRequestModel { repo, path };

    let response = FlUrl::new("/api/files/v1/folder")
        .execute_request(HttpVerb::Get, request)
        .await;

    super::handle_http_response(response).await
}

pub async fn get_content(repo: String, path: String) -> Result<FileContentResponse, RequestError> {
    let request = FileRequestModel { repo, path };

    let response = FlUrl::new("/api/files/v1/content")
        .execute_request(HttpVerb::Get, request)
        .await;

    super::handle_http_response(response).await
}

/// The url an `<img>` or an `<iframe>` is pointed at.
///
/// Built here rather than fetched, because these two tags are the one place the
/// browser does its own request — nothing in this crate ever reads the bytes.
pub fn raw_url(repo: &str, path: &str) -> String {
    format!(
        "/api/files/v1/raw?repo={}&path={}",
        encode_query_value(repo),
        encode_query_value(path)
    )
}

/// Percent-encodes everything that is not unreserved.
///
/// Hand-rolled because this crate has no url dependency and only needs the one
/// direction. Deliberately strict — a repository path can hold spaces, `#`,
/// `&`, `+` and non-ascii, every one of which silently truncates or corrupts
/// the query if it goes through unescaped.
fn encode_query_value(src: &str) -> String {
    let mut encoded = String::with_capacity(src.len());

    for byte in src.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_what_would_otherwise_end_the_query_early() {
        assert_eq!(encode_query_value("src/main.rs"), "src%2Fmain.rs");
        assert_eq!(encode_query_value("a b&c=d#e"), "a%20b%26c%3Dd%23e");
        // Two bytes in utf-8, so two escapes.
        assert_eq!(encode_query_value("é"), "%C3%A9");
    }
}
