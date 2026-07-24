use pulldown_cmark::{CowStr, Event, Options, Parser, Tag};

/// Schemes a destination is allowed to keep as it stands. Everything else — a
/// relative path, `javascript:`, `data:` — is rewritten or dropped below.
const SAFE_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "#"];

/// Turns a markdown file into the html the console drops straight into its own
/// DOM.
///
/// Which is exactly why this is not a plain `push_html` call. The file comes out
/// of a repository — content this server does not vouch for — and it lands on
/// the console's own origin, where script could read whatever authenticates the
/// reader. Two things are therefore neutralised here:
///
/// * **Raw html.** Markdown passes `<script>` through verbatim by design, so
///   the html events are dropped rather than emitted.
/// * **Destinations.** `[x](javascript:…)` survives as a link otherwise, and
///   runs on click. Only the schemes above are kept as they are.
///
/// A relative destination is rewritten to the raw file endpoint, so a README's
/// own screenshots actually appear instead of 404ing against the console's
/// routes — `dir` is the folder the markdown file itself sits in, relative to
/// the project root.
pub fn render_markdown(source: &str, repo: &str, dir: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let events = Parser::new_ext(source, options).filter_map(|event| match event {
        // Dropped, not escaped: showing the literal text of a `<script>` block
        // in the middle of a rendered page is noise, and it is never what the
        // author meant to be read.
        Event::Html(_) | Event::InlineHtml(_) => None,

        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Some(Event::Start(Tag::Link {
            link_type,
            dest_url: rewrite(dest_url, repo, dir),
            title,
            id,
        })),

        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Some(Event::Start(Tag::Image {
            link_type,
            dest_url: rewrite(dest_url, repo, dir),
            title,
            id,
        })),

        event => Some(event),
    });

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);

    html
}

/// Keeps a destination the browser can safely follow, points a relative one at
/// the file it names, and blanks anything else.
fn rewrite<'a>(dest_url: CowStr<'a>, repo: &str, dir: &str) -> CowStr<'a> {
    let trimmed = dest_url.trim();

    if trimmed.is_empty() {
        return dest_url;
    }

    let lowered = trimmed.to_lowercase();

    if SAFE_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme))
    {
        return dest_url;
    }

    // Anything else carrying a scheme is not a path — `javascript:`, `data:`,
    // `file:`, and whatever else a browser has been taught to handle. Blanked
    // rather than guessed at.
    if has_scheme(trimmed) {
        return CowStr::Borrowed("");
    }

    // A relative path, then. Resolved against the markdown file's own folder and
    // pointed at the raw endpoint — which re-checks confinement itself, so a
    // `../../etc/passwd` here is refused there rather than trusted from here.
    let (path, fragment) = split_fragment(trimmed);

    if path.is_empty() {
        return dest_url;
    }

    let joined = join(dir, path);

    let joined: Vec<String> = joined.split('/').map(|segment| encode(segment)).collect();

    CowStr::Boxed(
        format!("/raw/{}/{}{}", encode(repo), joined.join("/"), fragment).into_boxed_str(),
    )
}

/// True when the string starts with something of the shape `scheme:`.
///
/// Checked by hand rather than by looking for a `:`, because `notes:2024.md` is
/// a legal file name and a Windows-style `C:` is not a scheme either.
fn has_scheme(src: &str) -> bool {
    let colon = match src.find(':') {
        Some(colon) => colon,
        None => return false,
    };

    // A `:` after the first `/` belongs to the path, not to a scheme.
    if let Some(slash) = src.find('/') {
        if slash < colon {
            return false;
        }
    }

    let scheme = &src[..colon];

    // Per the url spec: a letter, then letters, digits, `+`, `-`, `.`.
    let mut chars = scheme.chars();

    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|itm| itm.is_ascii_alphanumeric() || itm == '+' || itm == '-' || itm == '.')
}

fn split_fragment(src: &str) -> (&str, &str) {
    match src.find('#') {
        Some(at) => (&src[..at], &src[at..]),
        None => (src, ""),
    }
}

/// Joins the markdown file's folder onto a relative destination, resolving the
/// `./` and `../` steps the author wrote.
///
/// Escaping the root is left possible on purpose — the raw endpoint runs the
/// result through the repository's own confinement, and that is the one place
/// this is decided.
fn join(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();

    let leading = if path.starts_with('/') { "" } else { dir };

    for part in leading.split('/').chain(path.split('/')) {
        match part {
            "" | "." => continue,
            // Kept once there is nothing left to climb out of, so the escape
            // reaches the confinement check rather than being silently
            // swallowed. Popping a `..` that is already there would do exactly
            // that swallowing — `../../x` would come out as `x`.
            ".." => match parts.last() {
                Some(last) if *last != ".." => {
                    parts.pop();
                }
                _ => parts.push(".."),
            },
            part => parts.push(part),
        }
    }

    parts.join("/")
}

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

    fn render(source: &str) -> String {
        render_markdown(source, "my-ssh", "docs")
    }

    #[test]
    fn renders_ordinary_markdown() {
        let html = render("# Title\n\nSome **bold** text.\n");

        assert!(html.contains("<h1>Title</h1>"), "{}", html);
        assert!(html.contains("<strong>bold</strong>"), "{}", html);
    }

    #[test]
    fn renders_the_extensions_a_readme_actually_uses() {
        let html = render("| a | b |\n| - | - |\n| 1 | 2 |\n");
        assert!(html.contains("<table>"), "{}", html);

        let html = render("~~gone~~");
        assert!(html.contains("<del>gone</del>"), "{}", html);
    }

    /// The reason this module exists rather than a plain `push_html`.
    #[test]
    fn raw_html_in_the_file_never_reaches_the_page() {
        let html = render("before\n\n<script>alert(1)</script>\n\nafter\n");

        assert!(!html.contains("<script"), "{}", html);
        assert!(html.contains("before"), "{}", html);
        assert!(html.contains("after"), "{}", html);

        // Inline, too — the same passthrough on one line.
        let html = render("text <img src=x onerror=alert(1)> more");
        assert!(!html.contains("onerror"), "{}", html);
    }

    #[test]
    fn a_script_url_is_not_left_clickable() {
        let html = render("[click](javascript:alert(1))");

        assert!(!html.to_lowercase().contains("javascript:"), "{}", html);
        assert!(html.contains("href=\"\""), "{}", html);

        // The other two a browser will happily execute.
        let html = render("[click](daTa:text/html;base64,PHNjcmlwdD4=)");
        assert!(!html.to_lowercase().contains("data:"), "{}", html);

        let html = render("![x](vbscript:msgbox)");
        assert!(!html.to_lowercase().contains("vbscript:"), "{}", html);
    }

    #[test]
    fn ordinary_links_are_left_alone() {
        let html = render("[docs](https://example.com/x) and [mail](mailto:a@b.c) and [top](#top)");

        assert!(html.contains("href=\"https://example.com/x\""), "{}", html);
        assert!(html.contains("href=\"mailto:a@b.c\""), "{}", html);
        assert!(html.contains("href=\"#top\""), "{}", html);
    }

    /// What makes a README's own screenshots show up instead of 404ing against
    /// the console's routes.
    #[test]
    fn a_relative_image_points_at_the_file_it_names() {
        let html = render("![shot](images/one.png)");

        assert!(
            html.contains("src=\"/raw/my-ssh/docs/images/one.png\""),
            "{}",
            html
        );
    }

    #[test]
    fn a_relative_link_resolves_against_the_markdown_files_own_folder() {
        // Up out of docs/, then down again.
        let html = render("[readme](../README.md)");
        assert!(html.contains("\"/raw/my-ssh/README.md\""), "{}", html);

        // Rooted at the project, not at the folder.
        let html = render("[x](/Cargo.toml)");
        assert!(html.contains("\"/raw/my-ssh/Cargo.toml\""), "{}", html);

        // The fragment stays a fragment rather than becoming part of the path.
        let html = render("[x](guide.md#install)");
        assert!(
            html.contains("\"/raw/my-ssh/docs/guide.md#install\""),
            "{}",
            html
        );

        // A space in a name is escaped, and the separators stay separators.
        // Angle brackets because that is how commonmark spells a destination
        // holding a space — without them it is not a link at all.
        let html = render("![x](<img/a b.png>)");
        assert!(
            html.contains("\"/raw/my-ssh/docs/img/a%20b.png\""),
            "{}",
            html
        );
    }

    #[test]
    fn tells_a_scheme_from_a_file_name_holding_a_colon() {
        assert!(has_scheme("javascript:alert(1)"));
        assert!(has_scheme("HTTPS://example.com"));
        assert!(has_scheme("data:text/html"));

        // `notes` is shaped like a scheme, so this reads as one — which is also
        // what a browser and what GitHub do with it. A file really named that
        // has to be linked as `./notes:2024.md`.
        assert!(has_scheme("notes:2024.md"));

        // Legal file names, not schemes: the `:` is past the first `/`, so it
        // belongs to the path.
        assert!(!has_scheme("docs/a:b.md"));
        assert!(!has_scheme("./x.md"));
        assert!(!has_scheme("plain.md"));
    }

    #[test]
    fn joins_a_relative_path_onto_the_folder() {
        assert_eq!(join("docs", "img/one.png"), "docs/img/one.png");
        assert_eq!(join("docs", "./one.png"), "docs/one.png");
        assert_eq!(join("docs/guide", "../one.png"), "docs/one.png");
        assert_eq!(join("", "one.png"), "one.png");

        // A leading slash means the project root rather than the folder.
        assert_eq!(join("docs", "/one.png"), "one.png");

        // Kept rather than swallowed, so the raw endpoint's confinement is what
        // refuses it.
        assert_eq!(join("", "../../etc/passwd"), "../../etc/passwd");
    }
}
