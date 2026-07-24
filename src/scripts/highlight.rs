use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Above this a file is served as plain text.
///
/// Well under the 4 MB the viewer will show at all: highlighting walks a grammar
/// over every byte, and on a generated file — a lock file, a bundled `.js`, a
/// vendored blob — that is seconds of cpu spent colouring something nobody
/// reads line by line. Plain text still arrives.
const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;

/// Every emitted class is prefixed, so the markup can not collide with the
/// console's own class names — `keyword` and `string` are exactly the kind of
/// word a stylesheet uses for something else.
fn class_style() -> ClassStyle {
    ClassStyle::SpacedPrefixed { prefix: "syn-" }
}

/// Loaded once. Deserializing the bundled grammar dump is the expensive part of
/// this module, and it does not depend on the file being looked at.
fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();

    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// One file's text as highlighted markup, or `None` to show it as it is.
///
/// `None` is a normal answer, not a failure: a file with no grammar behind it, a
/// generated file too big to be worth colouring, or a grammar that gave up
/// half-way all come back as plain text, which the viewer already knows how to
/// draw. Nothing here is worth failing a file open over.
///
/// The output is class-based rather than styled inline, which is what lets the
/// console's light and dark palettes both apply to it — `ClassedHTMLGenerator`
/// also escapes the source as it goes, so this is markup describing the file
/// rather than markup out of it.
pub fn highlight(text: &str, path: &str) -> Option<String> {
    if text.len() > MAX_HIGHLIGHT_BYTES {
        return None;
    }

    let syntaxes = syntaxes();

    // By extension first, because it is what a repository actually goes by. The
    // first line is the fallback that catches a shebang script named `deploy`
    // with no extension at all.
    let syntax = extension(path)
        .and_then(|extension| syntaxes.find_syntax_by_extension(&extension))
        .or_else(|| syntaxes.find_syntax_by_first_line(text.lines().next().unwrap_or_default()))?;

    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, syntaxes, class_style());

    for line in LinesWithEndings::from(text) {
        // A line the grammar chokes on drops the whole file back to plain text
        // rather than serving the half that parsed — a file that is coloured
        // down to line 400 and bare after it reads as corruption.
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }

    Some(generator.finalize())
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

    #[test]
    fn a_known_language_comes_back_marked_up() {
        let html = highlight("fn main() {}\n", "src/main.rs").unwrap();

        // The exact scopes are syntect's business; what this pins is that the
        // source was classified and escaped rather than passed through.
        assert!(html.contains("syn-"));
        assert!(html.contains("main"));
    }

    #[test]
    fn python_is_one_of_the_languages_that_ships() {
        let html = highlight("def main():\n    pass\n", "run.py").unwrap();

        assert!(html.contains("syn-"));
    }

    #[test]
    fn the_source_is_escaped_rather_than_emitted_as_markup() {
        // The viewer injects this string as html, so this is the property that
        // keeps a file from becoming part of the console.
        let html = highlight("let x = \"<script>alert(1)</script>\";\n", "a.rs").unwrap();

        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn a_file_with_no_grammar_behind_it_stays_plain() {
        assert!(highlight("just words\n", "notes.unknownext").is_none());
    }

    #[test]
    fn a_generated_file_too_big_to_be_worth_colouring_stays_plain() {
        let huge = "a\n".repeat(MAX_HIGHLIGHT_BYTES);

        assert!(highlight(&huge, "bundle.js").is_none());
    }

    #[test]
    fn an_extension_is_read_off_the_name_not_the_path() {
        assert_eq!(extension("src/main.rs").unwrap(), "rs");
        assert_eq!(extension("MAIN.PY").unwrap(), "py");

        assert!(extension("Makefile").is_none());
        assert!(extension(".gitignore").is_none());
    }
}
