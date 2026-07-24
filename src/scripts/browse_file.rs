use std::sync::Arc;

use crate::repo::RepoContext;

/// Above this a file is not decoded into the preview response at all. The
/// console renders text in one block, so a file bigger than this would freeze
/// the browser rather than show anything — saying "too big" is the useful
/// answer.
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;

/// Ceiling on what the raw endpoint will hand to the browser. Higher than the
/// text limit because an image is decoded by the browser, not by us.
const MAX_RAW_BYTES: u64 = 32 * 1024 * 1024;

/// Extensions the console shows in an `<img>`. Decided by extension rather than
/// by sniffing the bytes: an `.svg` is perfectly valid UTF-8, so a
/// "does it decode as text" test alone would render it as its own source.
const IMAGE_EXTENSIONS: [(&str, &str); 9] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("webp", "image/webp"),
    ("bmp", "image/bmp"),
    ("ico", "image/x-icon"),
    ("avif", "image/avif"),
];

const HTML_EXTENSIONS: [&str; 2] = ["html", "htm"];

const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

pub enum FilePreview {
    Text(String),
    /// The source and the markup it renders to. Both, because the console shows
    /// the rendering by default and lets the reader drop to the source without
    /// a second round trip.
    Markdown {
        source: String,
        html: String,
    },
    /// Fetched separately, as bytes, from the raw endpoint.
    Image,
    Html,
    /// The bytes are not valid UTF-8 and the extension names nothing the
    /// browser can render.
    Binary,
    TooBig,
}

pub struct FilePreviewResult {
    /// The path as the caller thinks about it — relative to the project root.
    pub path: String,
    pub size_bytes: u64,
    pub preview: FilePreview,
}

/// Decides how one file should be shown, and returns its text when the answer
/// is "as text".
///
/// "Is it text" is answered by actually decoding it — `String::from_utf8`, not
/// `from_utf8_lossy` — so a file that only half-decodes comes back as binary
/// rather than as a wall of replacement characters. `read_file` (the MCP tool)
/// deliberately does the opposite: an agent asking for a file wants whatever
/// text is in there. A human looking at a file wants to be told it is not text.
pub async fn preview_file(
    repo: &Arc<RepoContext>,
    path: &str,
) -> Result<FilePreviewResult, String> {
    let resolved = repo.resolve_path(path)?;
    let relative = repo.to_relative(&resolved);

    let metadata = tokio::fs::metadata(&resolved)
        .await
        .map_err(|err| format!("Can not read '{}'. Err: {}", relative, err))?;

    if metadata.is_dir() {
        return Err(format!("'{}' is a directory, not a file", relative));
    }

    let size_bytes = metadata.len();

    let done = |preview: FilePreview| {
        Ok(FilePreviewResult {
            path: relative.clone(),
            size_bytes,
            preview,
        })
    };

    if image_mime(&relative).is_some() {
        return done(FilePreview::Image);
    }

    if is_html(&relative) {
        return done(FilePreview::Html);
    }

    if size_bytes > MAX_TEXT_BYTES {
        return done(FilePreview::TooBig);
    }

    let content = tokio::fs::read(&resolved)
        .await
        .map_err(|err| format!("Can not read '{}'. Err: {}", relative, err))?;

    let text = match String::from_utf8(content) {
        // A NUL byte is valid UTF-8 but never occurs in a file anyone means to
        // read — it is the one cheap tell that separates a text file from a
        // binary that happens to decode.
        Ok(text) if !text.contains('\0') => text,
        _ => return done(FilePreview::Binary),
    };

    // Checked after the decode rather than beside the image and html
    // extensions: a `.md` that turns out not to be text at all is a binary
    // file with a misleading name, and rendering it as markdown would be
    // taking the name's word for it.
    if is_markdown(&relative) {
        let html = crate::scripts::render_markdown(&text, &repo.name, parent_of(&relative));

        return done(FilePreview::Markdown { source: text, html });
    }

    done(FilePreview::Text(text))
}

/// The folder a path sits in, relative to the project root — `""` for a file at
/// the root itself. What a markdown file's own relative links resolve against.
fn parent_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => "",
    }
}

pub struct RawFile {
    pub bytes: Vec<u8>,
    /// `None` when the extension names nothing known — the response then goes
    /// out without a content type rather than with a guessed one.
    pub content_type: Option<&'static str>,
}

/// The bytes of one file, for the `<img>` and `<iframe>` the console points at
/// it. Everything else in the console goes through JSON; this is the one
/// endpoint the browser fetches by itself.
pub async fn read_file_bytes(repo: &Arc<RepoContext>, path: &str) -> Result<RawFile, String> {
    let resolved = repo.resolve_path(path)?;
    let relative = repo.to_relative(&resolved);

    let metadata = tokio::fs::metadata(&resolved)
        .await
        .map_err(|err| format!("Can not read '{}'. Err: {}", relative, err))?;

    if metadata.is_dir() {
        return Err(format!("'{}' is a directory, not a file", relative));
    }

    if metadata.len() > MAX_RAW_BYTES {
        return Err(format!(
            "'{}' is {} bytes, which is above the {} byte limit for a browser preview",
            relative,
            metadata.len(),
            MAX_RAW_BYTES
        ));
    }

    let bytes = tokio::fs::read(&resolved)
        .await
        .map_err(|err| format!("Can not read '{}'. Err: {}", relative, err))?;

    Ok(RawFile {
        bytes,
        content_type: raw_content_type(&relative),
    })
}

/// Everything that is not an image and not html, keyed by extension.
///
/// Short of a full mime database on purpose — this list exists so that an html
/// page previewed in the console pulls in its own stylesheet, fonts and data,
/// which is the handful of types below. Anything else goes out with no type at
/// all, which with `nosniff` means the browser downloads it rather than
/// guessing.
const CONTENT_TYPES: [(&str, &str); 15] = [
    ("css", "text/css; charset=utf-8"),
    ("js", "text/javascript; charset=utf-8"),
    ("mjs", "text/javascript; charset=utf-8"),
    ("json", "application/json; charset=utf-8"),
    ("map", "application/json; charset=utf-8"),
    ("xml", "application/xml; charset=utf-8"),
    ("txt", "text/plain; charset=utf-8"),
    ("md", "text/plain; charset=utf-8"),
    ("csv", "text/csv; charset=utf-8"),
    ("wasm", "application/wasm"),
    ("pdf", "application/pdf"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
];

/// The `Content-Type` one file is served with, decided by its extension.
pub fn raw_content_type(path: &str) -> Option<&'static str> {
    if let Some(mime) = image_mime(path) {
        return Some(mime);
    }

    if is_html(path) {
        return Some("text/html; charset=utf-8");
    }

    let extension = extension(path)?;

    CONTENT_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime)| *mime)
}

fn image_mime(path: &str) -> Option<&'static str> {
    let extension = extension(path)?;

    IMAGE_EXTENSIONS
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime)| *mime)
}

fn is_html(path: &str) -> bool {
    match extension(path) {
        Some(extension) => HTML_EXTENSIONS.contains(&extension.as_str()),
        None => false,
    }
}

fn is_markdown(path: &str) -> bool {
    match extension(path) {
        Some(extension) => MARKDOWN_EXTENSIONS.contains(&extension.as_str()),
        None => false,
    }
}

/// The extension, lowercased. `None` for a name with no dot in it at all —
/// `Makefile` has no extension, and `.gitignore` is a name rather than an
/// extension of nothing.
fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?;
    let (stem, extension) = name.rsplit_once('.')?;

    if stem.is_empty() {
        return None;
    }

    Some(extension.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    #[test]
    fn reads_the_extension_off_the_last_component_only() {
        assert_eq!(extension("src/main.rs").as_deref(), Some("rs"));
        assert_eq!(extension("logo.PNG").as_deref(), Some("png"));
        assert_eq!(extension("a.b/Makefile"), None);
        // A dotfile is a name, not an extension.
        assert_eq!(extension(".gitignore"), None);
        assert_eq!(extension("README"), None);
    }

    #[test]
    fn knows_what_the_browser_can_render() {
        assert_eq!(image_mime("assets/logo.svg"), Some("image/svg+xml"));
        assert_eq!(image_mime("assets/photo.JPEG"), Some("image/jpeg"));
        assert_eq!(image_mime("src/main.rs"), None);

        assert!(is_html("wwwroot/index.html"));
        assert!(is_html("page.HTM"));
        assert!(!is_html("style.css"));
    }

    #[tokio::test]
    async fn a_source_file_comes_back_as_text() {
        let repo = build_test_repo("preview_text", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("main.rs"), "fn main() {}").unwrap();

        let result = preview_file(&repo, "main.rs").await.unwrap();

        match result.preview {
            FilePreview::Text(text) => assert_eq!(text, "fn main() {}"),
            _ => panic!("expected text"),
        }
    }

    /// The case `from_utf8_lossy` would silently turn into replacement
    /// characters and show as a wall of garbage.
    #[tokio::test]
    async fn bytes_which_are_not_utf8_come_back_as_binary() {
        let repo = build_test_repo("preview_binary", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("blob.dat"), [0xff, 0xfe, 0x00, 0x01]).unwrap();

        let result = preview_file(&repo, "blob.dat").await.unwrap();

        assert!(matches!(result.preview, FilePreview::Binary));
    }

    /// Valid UTF-8, so a decode test alone would call it text.
    #[tokio::test]
    async fn a_nul_byte_is_enough_to_call_it_binary() {
        let repo = build_test_repo("preview_nul", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("blob.bin"), b"MZ\0\0text").unwrap();

        let result = preview_file(&repo, "blob.bin").await.unwrap();

        assert!(matches!(result.preview, FilePreview::Binary));
    }

    /// An svg decodes as text perfectly well — the extension is what decides.
    #[tokio::test]
    async fn an_svg_is_an_image_rather_than_its_own_source() {
        let repo = build_test_repo("preview_svg", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("logo.svg"), "<svg></svg>").unwrap();

        let result = preview_file(&repo, "logo.svg").await.unwrap();

        assert!(matches!(result.preview, FilePreview::Image));
    }

    #[tokio::test]
    async fn html_is_rendered_rather_than_shown_as_text() {
        let repo = build_test_repo("preview_html", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("page.html"), "<h1>hi</h1>").unwrap();

        let result = preview_file(&repo, "page.html").await.unwrap();

        assert!(matches!(result.preview, FilePreview::Html));
    }

    #[tokio::test]
    async fn a_folder_is_not_a_file_to_preview() {
        let repo = build_test_repo("preview_dir", TestRepoOptions::default()).await;
        std::fs::create_dir_all(repo.root().join("src")).unwrap();

        assert!(preview_file(&repo, "src").await.is_err());
    }

    /// The confinement `resolve_path` enforces, checked here too because this is
    /// a path a browser hands in.
    #[tokio::test]
    async fn a_path_outside_the_root_is_refused() {
        let repo = build_test_repo("preview_escape", TestRepoOptions::default()).await;

        assert!(preview_file(&repo, "../../etc/passwd").await.is_err());
        assert!(read_file_bytes(&repo, "../../etc/passwd").await.is_err());
    }

    #[tokio::test]
    async fn raw_bytes_carry_the_content_type_the_browser_needs() {
        let repo = build_test_repo("raw_content_type", TestRepoOptions::default()).await;
        std::fs::write(repo.root().join("logo.svg"), "<svg></svg>").unwrap();
        std::fs::write(repo.root().join("main.rs"), "fn main() {}").unwrap();

        let image = read_file_bytes(&repo, "logo.svg").await.unwrap();
        assert_eq!(image.content_type, Some("image/svg+xml"));
        assert_eq!(image.bytes, b"<svg></svg>");

        // Nothing the browser renders — sent without a type rather than with a
        // guessed one.
        let source = read_file_bytes(&repo, "main.rs").await.unwrap();
        assert_eq!(source.content_type, None);
    }
}
