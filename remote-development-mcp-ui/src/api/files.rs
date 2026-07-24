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
///
/// The path form rather than the query form of the same endpoint, and that is
/// the whole reason it exists: a previewed html page asks for its own
/// stylesheet with a relative url, which the browser resolves against the
/// address the page came from. From `/raw/my-ssh/wwwroot/index.html`, `app.css`
/// beside it resolves to the file beside it. From a query url it would resolve
/// back onto the api route and arrive as nothing.
pub fn raw_url(repo: &str, path: &str) -> String {
    let path: Vec<String> = path.split('/').map(|segment| encode(segment)).collect();

    format!("/raw/{}/{}", encode(repo), path.join("/"))
}

/// Percent-encodes one path segment.
///
/// Hand-rolled because this crate has no url dependency and only needs the one
/// direction. Deliberately strict — a repository path can hold spaces, `#`,
/// `?`, `%` and non-ascii, every one of which silently truncates or corrupts
/// the url if it goes through unescaped.
fn encode(src: &str) -> String {
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
    fn keeps_the_separators_and_escapes_the_rest() {
        assert_eq!(raw_url("my-ssh", "src/main.rs"), "/raw/my-ssh/src/main.rs");

        // The separators survive as separators; everything else is escaped, so
        // a `?` in a name can not start a query string.
        assert_eq!(
            raw_url("my-ssh", "docs/a b?c.md"),
            "/raw/my-ssh/docs/a%20b%3Fc.md"
        );

        // Two bytes in utf-8, so two escapes.
        assert_eq!(raw_url("my-ssh", "é.md"), "/raw/my-ssh/%C3%A9.md");
    }
}
