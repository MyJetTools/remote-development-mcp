use std::sync::Arc;

use crate::repo::RepoContext;

/// Refuses to slurp something enormous into memory. Anything bigger wants
/// `search` or a `run_command` with the right tool, not a whole-file read.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

pub const DEFAULT_LINE_LIMIT: usize = 2000;

/// Upper bound on a single read, so a caller-supplied `limit` can not drive an
/// unbounded slice. Pages larger than this simply come back in several reads.
const MAX_LINE_LIMIT: usize = 50_000;

pub struct ReadFileResult {
    pub content: String,
    pub total_lines: usize,
    /// True when the returned window does not cover the whole file.
    pub truncated: bool,
}

/// Reads a file as text, windowed by line.
///
/// `offset` is a 1-based line number, matching how the rest of the world talks
/// about lines in a file.
pub async fn read_file(
    repo: &Arc<RepoContext>,
    path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<ReadFileResult, String> {
    let resolved = repo.resolve_path(path)?;

    let metadata = tokio::fs::metadata(&resolved).await.map_err(|err| {
        format!(
            "Can not read '{}'. Err: {}",
            repo.to_relative(&resolved),
            err
        )
    })?;

    if metadata.is_dir() {
        return Err(format!(
            "'{}' is a directory. Use list_dir for it",
            repo.to_relative(&resolved)
        ));
    }

    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "'{}' is {} bytes, which is above the {} byte read limit. Use search to find what you \
             need inside it",
            repo.to_relative(&resolved),
            metadata.len(),
            MAX_FILE_BYTES
        ));
    }

    let content = tokio::fs::read(&resolved).await.map_err(|err| {
        format!(
            "Can not read '{}'. Err: {}",
            repo.to_relative(&resolved),
            err
        )
    })?;

    let content = String::from_utf8_lossy(&content);

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let offset = match offset {
        Some(offset) => offset.max(1),
        None => 1,
    };

    let limit = match limit {
        Some(limit) => limit.clamp(1, MAX_LINE_LIMIT),
        None => DEFAULT_LINE_LIMIT,
    };

    let start = offset - 1;

    if start >= total_lines {
        return Ok(ReadFileResult {
            content: String::new(),
            total_lines,
            // Nothing after the window means nothing left to page for. Reporting
            // truncated here would make a "keep reading while truncated" loop
            // spin forever on empty responses.
            truncated: false,
        });
    }

    // saturating_add so a caller-supplied offset/limit can never overflow into a
    // reversed, panicking slice range.
    let end = start.saturating_add(limit).min(total_lines);

    Ok(ReadFileResult {
        content: lines[start..end].join("\n"),
        total_lines,
        // Purely "there are lines past this window" — true for a mid-file page,
        // false once the window reaches the end, so paging terminates.
        truncated: end < total_lines,
    })
}
