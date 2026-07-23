use std::sync::Arc;

use super::{ActivityEvent, ActivityLog};

/// How many frames of our own code go into the console line. The whole
/// backtrace is hundreds of frames, nearly all of them runtime internals; the
/// few that name this crate are the ones that say where to look.
const FRAMES_SHOWN: usize = 4;

/// Routes every panic in the process into the console feed.
///
/// A panic in a spawned task does not stop the server — the task dies and
/// everything else carries on — so without this it would vanish silently and the
/// only symptom would be a job that never finishes.
///
/// The default report still goes to stderr afterwards, and that is the only
/// thing this server prints once it is up: the terminal keeps the full
/// backtrace with nothing streaming past it, while the browser console gets the
/// one-line version in place, alongside whatever was happening around it.
pub fn install(activity: Arc<ActivityLog>) {
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let location = match info.location() {
            Some(location) => format!("{}:{}", location.file(), location.line()),
            None => "unknown location".to_string(),
        };

        let backtrace = std::backtrace::Backtrace::force_capture().to_string();

        activity.push(ActivityEvent::panicked(
            location,
            panic_message(info),
            own_frames(&backtrace),
        ));

        previous(info);
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    // The payload is `&str` for `panic!("literal")` and `String` for a formatted
    // one; anything else carries nothing worth printing.
    if let Some(message) = info.payload().downcast_ref::<&str>() {
        return (*message).to_string();
    }

    if let Some(message) = info.payload().downcast_ref::<String>() {
        return message.clone();
    }

    "panicked".to_string()
}

/// Keeps only the frames that name this crate, in order, and renders them on one
/// line so a panic occupies a single row of the console.
fn own_frames(backtrace: &str) -> String {
    let frames: Vec<String> = backtrace
        .lines()
        .filter(|line| line.contains("remote_development_mcp"))
        .map(clean_frame)
        .filter(|frame| !frame.is_empty())
        .take(FRAMES_SHOWN)
        .collect();

    frames.join(" ← ")
}

/// `   5: remote_development_mcp::scripts::run_command::h1a2b` becomes
/// `scripts::run_command`.
fn clean_frame(line: &str) -> String {
    let without_index = match line.split_once(": ") {
        Some((_index, rest)) => rest,
        None => line,
    };

    let name = without_index.trim();

    let name = name
        .strip_prefix("remote_development_mcp::")
        .unwrap_or(name)
        .trim();

    // Drop the trailing hash symbol rustc appends to a mangled name.
    match name.rsplit_once("::h") {
        Some((head, tail)) if tail.len() >= 16 && tail.chars().all(|c| c.is_ascii_hexdigit()) => {
            head.to_string()
        }
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both halves of the contract, in one test on purpose: the panic hook is
    /// process-global, so two tests installing it would race each other.
    ///
    /// Half one — a real panic reaches the feed the browser reads. Half two —
    /// the hook that was already installed still runs, which is what keeps the
    /// default backtrace going to the terminal. Now that nothing else is
    /// printed, that report is the whole of what the terminal is for.
    #[test]
    fn a_real_panic_reaches_both_the_feed_and_the_hook_behind_us() {
        let activity = Arc::new(ActivityLog::new());

        let previous_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));

        {
            let previous_ran = previous_ran.clone();
            std::panic::set_hook(Box::new(move |_| {
                previous_ran.store(true, std::sync::atomic::Ordering::SeqCst);
            }));
        }

        install(activity.clone());

        let result = std::panic::catch_unwind(|| {
            panic!("a-panic-only-this-test-raises");
        });

        assert!(result.is_err());

        assert!(
            previous_ran.load(std::sync::atomic::Ordering::SeqCst),
            "the hook installed before ours did not run — the backtrace would \
             never reach the terminal"
        );

        let landed = activity
            .recent(usize::MAX)
            .into_iter()
            .find(|event| event.detail.contains("a-panic-only-this-test-raises"));

        let landed = landed.expect("the panic did not reach the activity feed");

        assert_eq!(landed.kind, crate::activity::ActivityKind::Panicked);
        // The location is what a reader needs first — which file and line.
        assert!(
            landed.subject.contains("panic_hook.rs"),
            "{}",
            landed.subject
        );
    }

    #[test]
    fn keeps_only_our_own_frames_and_shortens_them() {
        let backtrace = "\
   0: std::backtrace::Backtrace::force_capture
   1: core::panicking::panic_fmt
   2: remote_development_mcp::scripts::run_command::supervise_job::h0123456789abcdef
   3: tokio::runtime::task::core::Core::poll
   4: remote_development_mcp::jobs::job_log::pump_stream::hfedcba9876543210
   5: tokio::runtime::scheduler::multi_thread::worker::run";

        let rendered = own_frames(backtrace);

        assert_eq!(
            rendered,
            "scripts::run_command::supervise_job ← jobs::job_log::pump_stream"
        );
        assert!(!rendered.contains("tokio"));
        assert!(!rendered.contains('\n'));
    }

    #[test]
    fn a_backtrace_with_none_of_our_frames_renders_as_nothing() {
        let backtrace = "   0: core::panicking::panic_fmt\n   1: tokio::runtime::task";

        assert_eq!(own_frames(backtrace), "");
    }

    #[test]
    fn only_the_first_few_frames_are_kept() {
        let backtrace = (0..20)
            .map(|index| format!("   {}: remote_development_mcp::frame_{}", index, index))
            .collect::<Vec<_>>()
            .join("\n");

        let rendered = own_frames(&backtrace);

        assert_eq!(rendered.matches('←').count(), FRAMES_SHOWN - 1);
    }

    #[test]
    fn a_frame_without_an_index_prefix_still_cleans_up() {
        assert_eq!(
            clean_frame("remote_development_mcp::scripts::search"),
            "scripts::search"
        );
    }

    #[test]
    fn a_name_that_merely_contains_h_is_not_truncated() {
        // `::hash` is not a mangling suffix — only a long hex tail is.
        assert_eq!(
            clean_frame("   3: remote_development_mcp::utils::hash"),
            "utils::hash"
        );
    }
}
