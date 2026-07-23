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
}
