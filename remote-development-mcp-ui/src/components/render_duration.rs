/// Seconds as something a person reads at a glance: `4s`, `1m12s`, `2h03m`.
pub fn render_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;

    if total < 60 {
        return format!("{}s", total);
    }

    if total < 3600 {
        return format!("{}m{:02}s", total / 60, total % 60);
    }

    format!("{}h{:02}m", total / 3600, (total % 3600) / 60)
}

/// Like [`render_duration`], but keeps a tenth of a second under ten seconds.
///
/// The history feed times synchronous tool calls, which usually finish in a
/// fraction of a second — rounded to whole seconds they would all read `0s` and
/// the column would say nothing. Above ten seconds the fraction stops mattering
/// and this reads exactly like `render_duration`.
pub fn render_precise_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);

    if seconds < 10.0 {
        return format!("{:.1}s", seconds);
    }

    render_duration(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_minutes_and_hours_each_read_differently() {
        assert_eq!(render_duration(4.7), "4s");
        assert_eq!(render_duration(72.0), "1m12s");
        assert_eq!(render_duration(7380.0), "2h03m");
    }

    #[test]
    fn a_negative_duration_does_not_wrap_around() {
        assert_eq!(render_duration(-1.0), "0s");
    }

    #[test]
    fn a_sub_second_call_keeps_its_tenth_rather_than_reading_zero() {
        assert_eq!(render_precise_duration(0.05), "0.1s");
        assert_eq!(render_precise_duration(0.0), "0.0s");
        assert_eq!(render_precise_duration(3.4), "3.4s");
    }

    #[test]
    fn a_long_duration_drops_the_fraction_like_the_plain_form() {
        assert_eq!(render_precise_duration(72.0), "1m12s");
        assert_eq!(render_precise_duration(11.0), "11s");
    }
}
