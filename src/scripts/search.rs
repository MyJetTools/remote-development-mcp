use std::sync::Arc;

use crate::repo::RepoContext;

use super::exec_capture;

pub const DEFAULT_MAX_RESULTS: usize = 200;

/// Ceiling on a caller-supplied `max_results`, so it can not drive an unbounded
/// result set. Narrow the pattern rather than asking for more than this.
const MAX_RESULTS: usize = 5000;

pub struct SearchRequest {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub max_results: Option<usize>,
    pub ignore_case: bool,
}

pub struct SearchMatch {
    pub file: String,
    pub line: u64,
    pub text: String,
}

pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

/// Content search through ripgrep.
///
/// ripgrep is used rather than a hand-rolled walk because it already honours
/// `.gitignore`, skips binaries, and is fast enough to make searching a large
/// monorepo practical — which is the whole reason this tool exists.
///
/// Its `--json` output is parsed instead of the human format: a match line
/// contains arbitrary text, and splitting `file:line:text` on colons breaks the
/// moment the match itself contains one.
pub async fn search(
    repo: &Arc<RepoContext>,
    request: SearchRequest,
) -> Result<SearchResult, String> {
    if request.pattern.trim().is_empty() {
        return Err("The search pattern is empty".to_string());
    }

    let search_root = match request.path.as_ref() {
        Some(path) => repo.resolve_path(path)?,
        None => repo.root().to_path_buf(),
    };

    let max_results = request
        .max_results
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, MAX_RESULTS);

    let search_root = search_root.to_string_lossy().to_string();

    let mut args: Vec<&str> = vec!["--json", "--line-number"];

    if request.ignore_case {
        args.push("--ignore-case");
    }

    if let Some(glob) = request.glob.as_ref() {
        args.push("--glob");
        args.push(glob.as_str());
    }

    // `-e` and `--` keep a pattern that starts with a dash from being read as a
    // flag.
    args.push("-e");
    args.push(request.pattern.as_str());
    args.push("--");
    args.push(search_root.as_str());

    let output = exec_capture("rg", &args, repo.root(), None).await?;

    // ripgrep exits 1 when it simply found nothing; only above that is a real
    // failure.
    let found_nothing = output.exit_code == Some(1);

    if !output.success && !found_nothing {
        return Err(format!("Search failed. Err: {}", output.stderr.trim()));
    }

    let mut matches = Vec::new();
    let mut truncated = false;

    for line in output.stdout.lines() {
        let found = match parse_rg_event(line) {
            Some(found) => found,
            // `begin`, `end` and `summary` events carry no match; skipping them
            // here is what keeps the cap check below counting real matches only.
            None => continue,
        };

        // Tested only against parsed matches, so the count of `end`/`summary`
        // events ripgrep emits after the last hit can no longer push it over
        // the cap and report a truncation that did not happen.
        if matches.len() >= max_results {
            truncated = true;
            break;
        }

        matches.push(SearchMatch {
            file: repo.to_relative(std::path::Path::new(&found.file)),
            line: found.line,
            text: found.text,
        });
    }

    Ok(SearchResult { matches, truncated })
}

/// One `--json` event from ripgrep, before the path is made repository-relative.
struct RawMatch {
    file: String,
    line: u64,
    text: String,
}

/// ripgrep emits `begin`, `match`, `end` and `summary` events on the same
/// stream; only `match` carries a hit.
fn parse_rg_event(line: &str) -> Option<RawMatch> {
    let parsed: serde_json::Value = serde_json::from_str(line).ok()?;

    if parsed.get("type")?.as_str()? != "match" {
        return None;
    }

    let data = parsed.get("data")?;

    // A path ripgrep could not decode arrives as `bytes` instead of `text`;
    // there is nothing useful to show for it.
    let file = data.get("path")?.get("text")?.as_str()?;
    let line_number = data.get("line_number")?.as_u64()?;
    let text = data
        .get("lines")
        .and_then(|lines| lines.get("text"))
        .and_then(|text| text.as_str())
        .unwrap_or_default();

    Some(RawMatch {
        file: file.to_string(),
        line: line_number,
        text: text.trim_end_matches('\n').to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_file_line_and_text_out_of_a_match_event() {
        let event = r#"{"type":"match","data":{"path":{"text":"/repo/src/main.rs"},"lines":{"text":"let url = \"a:b:c\";\n"},"line_number":42}}"#;

        let parsed = parse_rg_event(event).unwrap();

        assert_eq!(parsed.file, "/repo/src/main.rs");
        assert_eq!(parsed.line, 42);
        // The reason for parsing JSON rather than splitting on ':'.
        assert_eq!(parsed.text, "let url = \"a:b:c\";");
    }

    #[test]
    fn ignores_events_which_are_not_matches() {
        assert!(parse_rg_event(r#"{"type":"begin","data":{"path":{"text":"/a.rs"}}}"#).is_none());
        assert!(parse_rg_event(r#"{"type":"end","data":{"path":{"text":"/a.rs"}}}"#).is_none());
        assert!(parse_rg_event(r#"{"type":"summary","data":{}}"#).is_none());
    }

    #[test]
    fn ignores_a_line_which_is_not_json_at_all() {
        assert!(parse_rg_event("rg: something went wrong").is_none());
        assert!(parse_rg_event("").is_none());
    }

    #[test]
    fn skips_a_match_whose_path_could_not_be_decoded() {
        let event = r#"{"type":"match","data":{"path":{"bytes":"3q2+7w=="},"lines":{"text":"x\n"},"line_number":1}}"#;

        assert!(parse_rg_event(event).is_none());
    }
}
