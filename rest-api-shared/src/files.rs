use my_http_utils::macros::{MyHttpInput, MyHttpObjectStructure};
use serde::{Deserialize, Serialize};

#[derive(MyHttpInput)]
pub struct ListFolderRequestModel {
    #[http_query(name = "repo", description = "Project id the folder belongs to")]
    pub repo: String,

    #[http_query(
        name = "path",
        description = "Folder relative to the project root. Left out for the root itself"
    )]
    pub path: Option<String>,
}

#[derive(MyHttpInput)]
pub struct FileRequestModel {
    #[http_query(name = "repo", description = "Project id the file belongs to")]
    pub repo: String,

    #[http_query(name = "path", description = "File relative to the project root")]
    pub path: String,
}

/// One row of a folder listing.
#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct FolderEntryModel {
    /// The last component only — what the tree renders.
    pub name: String,
    /// The whole path relative to the project root, which is what every other
    /// call takes. Kept alongside `name` so the tree never has to join paths
    /// itself and can not get the separator wrong.
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct ListFolderResponse {
    /// The folder that was listed, relative to the root. Empty for the root
    /// itself.
    pub path: String,
    pub entries: Vec<FolderEntryModel>,
    /// True when the folder holds more entries than one listing returns.
    pub truncated: bool,
}

/// What [`FileContentResponse::kind`] can say. A string rather than an enum
/// because that is how the rest of this contract carries closed sets, and
/// because a `kind` the console does not recognise should render as "can not be
/// shown" rather than fail to deserialize the whole response.
pub const FILE_KIND_TEXT: &str = "text";
pub const FILE_KIND_MARKDOWN: &str = "markdown";
pub const FILE_KIND_IMAGE: &str = "image";
pub const FILE_KIND_HTML: &str = "html";
/// Bytes the browser renders itself, like html — but a distinct kind, because
/// they are not the same thing to anything that reasons about the file.
pub const FILE_KIND_PDF: &str = "pdf";
/// Too big to decode and ship through this JSON, but small enough for the raw
/// endpoint — so the browser fetches it and renders it itself. The console draws
/// it in the same frame as html and pdf.
pub const FILE_KIND_BROWSER: &str = "browser";
pub const FILE_KIND_BINARY: &str = "binary";
pub const FILE_KIND_TOO_BIG: &str = "too-big";

#[derive(Serialize, Deserialize, MyHttpObjectStructure, Clone, Debug, PartialEq)]
pub struct FileContentResponse {
    pub path: String,
    pub size_bytes: u64,
    /// One of the `FILE_KIND_*` constants. Decided by the server, not by the
    /// console: whether a file is text is whether its bytes actually decoded as
    /// UTF-8, which only the side holding the bytes can answer.
    pub kind: String,
    /// The file, when `kind` is `text` or `markdown`. Absent for every other
    /// kind — an image and an html page are fetched as bytes from the raw
    /// endpoint instead, so they are never base64'd through this JSON.
    pub text: Option<String>,
    /// The rendered markup, when `kind` is `markdown`. Sent alongside the source
    /// rather than instead of it, so the console can show either without asking
    /// for the file twice.
    pub html: Option<String>,
}
