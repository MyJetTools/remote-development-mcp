use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// A slice of a job log, addressed by a byte offset into the log file.
///
/// Byte offsets into a file — rather than an in-memory ring buffer — are what
/// makes polling hole-free: a client that comes back late still resumes exactly
/// where it left off, however far behind it fell.
pub struct JobLogChunk {
    pub text: String,
    pub next_cursor: u64,
    /// More output is already available past `next_cursor`; poll again.
    pub has_more: bool,
}

/// Small enough to be cheap, large enough that a multi-byte character can never
/// fill a whole read window.
pub const MIN_READ_BYTES: u64 = 1024;
pub const DEFAULT_READ_BYTES: u64 = 64 * 1024;
pub const MAX_READ_BYTES: u64 = 4 * 1024 * 1024;

const TRUNCATION_MARKER: &[u8] = b"\n--- output truncated: max_log_bytes reached ---\n";

pub fn clamp_read_bytes(requested: Option<u64>) -> u64 {
    let requested = match requested {
        Some(requested) => requested,
        None => DEFAULT_READ_BYTES,
    };

    requested.clamp(MIN_READ_BYTES, MAX_READ_BYTES)
}

pub async fn read_log_at(path: &Path, cursor: u64, max_bytes: u64) -> Result<JobLogChunk, String> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                return Ok(JobLogChunk {
                    text: String::new(),
                    next_cursor: cursor,
                    has_more: false,
                });
            }

            return Err(format!(
                "Can not open job log '{}'. Err: {}",
                path.display(),
                err
            ));
        }
    };

    let len = file
        .metadata()
        .await
        .map_err(|err| {
            format!(
                "Can not read job log metadata '{}'. Err: {}",
                path.display(),
                err
            )
        })?
        .len();

    if cursor >= len {
        return Ok(JobLogChunk {
            text: String::new(),
            next_cursor: len,
            has_more: false,
        });
    }

    file.seek(std::io::SeekFrom::Start(cursor))
        .await
        .map_err(|err| {
            format!(
                "Can not seek job log '{}' to {}. Err: {}",
                path.display(),
                cursor,
                err
            )
        })?;

    let to_read = std::cmp::min(max_bytes, len - cursor);

    let mut buffer = vec![0u8; to_read as usize];

    file.read_exact(&mut buffer).await.map_err(|err| {
        format!(
            "Can not read job log '{}' at {}. Err: {}",
            path.display(),
            cursor,
            err
        )
    })?;

    let (text, consumed) = decode_to_char_boundary(&buffer);

    let next_cursor = cursor + consumed as u64;

    Ok(JobLogChunk {
        text,
        next_cursor,
        has_more: next_cursor < len,
    })
}

/// Decodes as much of `buffer` as ends on a character boundary, returning the
/// text and how many bytes it consumed.
///
/// A multi-byte character cut by the end of the window — including the end of
/// the file for a log still being written — is left for the next call rather
/// than turned into `U+FFFD`; consuming its lead byte would lose the character
/// once the rest arrives. Genuinely invalid bytes are stepped over lossily so a
/// log containing real garbage still makes progress.
fn decode_to_char_boundary(buffer: &[u8]) -> (String, usize) {
    match std::str::from_utf8(buffer) {
        Ok(text) => (text.to_string(), buffer.len()),
        Err(err) => {
            let valid_up_to = err.valid_up_to();

            match err.error_len() {
                // Genuinely invalid bytes at `valid_up_to`: emit the valid
                // prefix plus a replacement and step past them. `invalid_len`
                // is at least one, so this always makes progress.
                Some(invalid_len) => {
                    let consumed = valid_up_to + invalid_len;
                    (
                        String::from_utf8_lossy(&buffer[..consumed]).into_owned(),
                        consumed,
                    )
                }
                // The tail is only the head of a character. Emit the complete
                // prefix and leave the rest — even when that means consuming
                // nothing this time (the window is entirely one partial
                // character), which resolves as soon as more bytes are written.
                None => (
                    String::from_utf8_lossy(&buffer[..valid_up_to]).into_owned(),
                    valid_up_to,
                ),
            }
        }
    }
}

/// Drains one of the child's streams into its log file.
///
/// Once `max_bytes` is reached the log stops growing, but the stream keeps
/// being drained: a child whose pipe fills up blocks forever, so a capped log
/// must never turn into a stalled build.
pub async fn pump_stream<TRead: AsyncRead + Unpin>(
    mut reader: TRead,
    path: PathBuf,
    max_bytes: u64,
) {
    let mut file = match tokio::fs::File::create(&path).await {
        Ok(file) => file,
        Err(err) => {
            my_logger::LOGGER.write_error(
                "pump_stream",
                format!("Can not create job log. Err: {:?}", err),
                my_logger::LogEventCtx::new().add("path", path.display().to_string()),
            );
            drain(&mut reader).await;
            return;
        }
    };

    let mut buffer = vec![0u8; 8 * 1024];
    let mut written: u64 = 0;
    let mut capped = false;

    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) => {
                my_logger::LOGGER.write_error(
                    "pump_stream",
                    format!("Can not read child output. Err: {:?}", err),
                    my_logger::LogEventCtx::new().add("path", path.display().to_string()),
                );
                break;
            }
        };

        if capped {
            continue;
        }

        let room = max_bytes.saturating_sub(written);

        let take = std::cmp::min(room as usize, read);

        // Mark the cap the moment bytes are actually dropped — at this chunk,
        // not the next iteration. Doing it later loses the marker entirely when
        // the straddling chunk is the last one before EOF, and the log then
        // looks complete while silently missing its tail.
        if take < read {
            capped = true;
        }

        if let Err(err) = file.write_all(&buffer[..take]).await {
            my_logger::LOGGER.write_error(
                "pump_stream",
                format!("Can not write job log. Err: {:?}", err),
                my_logger::LogEventCtx::new().add("path", path.display().to_string()),
            );
            break;
        }

        written += take as u64;

        // Written right after the bytes that hit the cap, so it is never lost to
        // an EOF on the very next read.
        if capped {
            let _ = file.write_all(TRUNCATION_MARKER).await;
        }

        // Flushed on every chunk on purpose: the log is the polling surface, so
        // output that is buffered is output the client can not see yet.
        if let Err(err) = file.flush().await {
            my_logger::LOGGER.write_error(
                "pump_stream",
                format!("Can not flush job log. Err: {:?}", err),
                my_logger::LogEventCtx::new().add("path", path.display().to_string()),
            );
            break;
        }
    }

    let _ = file.flush().await;
}

async fn drain<TRead: AsyncRead + Unpin>(reader: &mut TRead) {
    let mut buffer = vec![0u8; 8 * 1024];

    while let Ok(read) = reader.read(&mut buffer).await {
        if read == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("remote-development-mcp-tests-logs");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn decodes_whole_buffer_when_it_is_valid() {
        let (text, consumed) = decode_to_char_boundary("hello".as_bytes());

        assert_eq!(text, "hello");
        assert_eq!(consumed, 5);
    }

    #[test]
    fn leaves_a_character_split_by_the_window_for_the_next_read() {
        // "п" is two bytes; cut it in half.
        let full = "aп".as_bytes().to_vec();
        let (text, consumed) = decode_to_char_boundary(&full[..full.len() - 1]);

        assert_eq!(text, "a");
        assert_eq!(consumed, 1);
    }

    #[test]
    fn a_window_holding_only_a_partial_character_consumes_nothing() {
        // Just the lead byte of "п" — nothing is decodable yet, and consuming it
        // would lose the character once its second byte is written.
        let lead = &"п".as_bytes()[..1];

        let (text, consumed) = decode_to_char_boundary(lead);

        assert_eq!(text, "");
        assert_eq!(consumed, 0);
    }

    #[test]
    fn a_multibyte_character_split_by_a_growing_file_survives_the_next_read() {
        // The window ends inside "п"; the rest arrives on the following read.
        let full = "café п".as_bytes().to_vec();

        let cut = full.len() - 1;
        let (first, consumed) = decode_to_char_boundary(&full[..cut]);

        // Everything up to the split, and not the split character's lead byte.
        assert_eq!(first, "café ");
        assert!(consumed < cut);

        let (second, _) = decode_to_char_boundary(&full[consumed..]);

        assert_eq!(format!("{}{}", first, second), "café п");
    }

    #[test]
    fn always_makes_progress_on_invalid_bytes() {
        let (_, consumed) = decode_to_char_boundary(&[0xFF, 0xFE]);

        assert!(consumed > 0);
    }

    #[tokio::test]
    async fn missing_log_reads_as_empty() {
        let path = temp_log("missing.log");

        let chunk = read_log_at(&path, 0, DEFAULT_READ_BYTES).await.unwrap();

        assert_eq!(chunk.text, "");
        assert_eq!(chunk.next_cursor, 0);
        assert!(!chunk.has_more);
    }

    #[tokio::test]
    async fn cursor_walk_reassembles_the_log_without_holes() {
        let path = temp_log("cursor_walk.log");

        let content = "строка один\nline two\nстрока три\n".repeat(200);
        tokio::fs::write(&path, content.as_bytes()).await.unwrap();

        let mut cursor = 0u64;
        let mut collected = String::new();

        loop {
            // A deliberately awkward window size, to land inside multi-byte
            // characters as often as possible.
            let chunk = read_log_at(&path, cursor, MIN_READ_BYTES).await.unwrap();

            collected.push_str(&chunk.text);
            cursor = chunk.next_cursor;

            if !chunk.has_more {
                break;
            }
        }

        assert_eq!(collected, content);
        assert_eq!(cursor, content.len() as u64);
    }

    #[tokio::test]
    async fn reading_past_the_end_is_empty_and_not_an_error() {
        let path = temp_log("past_end.log");
        tokio::fs::write(&path, b"abc").await.unwrap();

        let chunk = read_log_at(&path, 100, DEFAULT_READ_BYTES).await.unwrap();

        assert_eq!(chunk.text, "");
        assert_eq!(chunk.next_cursor, 3);
        assert!(!chunk.has_more);
    }

    #[tokio::test]
    async fn pump_caps_the_log_and_keeps_draining() {
        let path = temp_log("capped.log");

        let payload = vec![b'x'; 100 * 1024];

        pump_stream(payload.as_slice(), path.clone(), 1024).await;

        let written = tokio::fs::read(&path).await.unwrap();

        assert!(written.len() < payload.len(), "log should have been capped");
        assert!(written.ends_with(TRUNCATION_MARKER));
    }

    #[tokio::test]
    async fn pump_writes_everything_below_the_cap() {
        let path = temp_log("uncapped.log");

        pump_stream(&b"hello world"[..], path.clone(), 1024).await;

        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"hello world");
    }
}
