//! Rendering UTC instants in the viewer's local timezone.
//!
//! The server sends every instant as a raw number — Unix microseconds — and, on
//! each snapshot, its own clock as `server_time`. The browser pairs that with
//! its own clock to derive the offset once ([`timezone_from_snapshot`]); every
//! instant is then rebuilt with `DateTimeAsMicroseconds::new` and rendered
//! through that offset. The server never learns where the viewer is — the whole
//! localisation happens here.

use rust_extensions::date_time::{
    DateTimeAsMicroseconds, DateTimeAsMicrosecondsWithTimeZone, TimeZone,
};

/// The viewer's timezone, derived from the server's clock and the browser's.
///
/// `server_time` is the UTC instant the snapshot was taken, in microseconds; the
/// browser's local wall-clock for *now* is read here. Their difference is the
/// offset — the same `from_server_and_local_time` the server-side code would
/// use, rounded to the nearest 15 minutes. A network delay of a few
/// milliseconds between the two reads is far below that rounding, so it cannot
/// shift the zone.
pub fn timezone_from_snapshot(server_time: i64) -> TimeZone {
    let server = DateTimeAsMicroseconds::new(server_time);

    // The browser's wall clock for now, encoded as if it were UTC — exactly what
    // `from_server_and_local_time` wants for its `local_time`, so the difference
    // is the viewer's offset. dioxus-utils reads it (`Date.now()` shifted by
    // `getTimezoneOffset`), which is the browser's own authority on its zone.
    let local = dioxus_utils::now_local_date_time();

    TimeZone::from_server_and_local_time(server, local)
}

/// `HH:MM:SS` in the viewer's zone. The feed shows a live stream, so the date is
/// noise — only the time of day is rendered.
pub fn local_time_of_day(micros: i64, tz: TimeZone) -> String {
    let local = with_zone(micros, tz).to_local_date_time_struct();

    format!(
        "{:02}:{:02}:{:02}",
        local.time.hour, local.time.min, local.time.sec
    )
}

/// `YYYY-MM-DD HH:MM:SS` in the viewer's zone — the full instant, for a tooltip
/// where the relative "N ago" is not precise enough.
pub fn local_date_time(micros: i64, tz: TimeZone) -> String {
    with_zone(micros, tz).to_compact_string()
}

fn with_zone(micros: i64, tz: TimeZone) -> DateTimeAsMicrosecondsWithTimeZone {
    DateTimeAsMicrosecondsWithTimeZone::new(DateTimeAsMicroseconds::new(micros), tz)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The browser-clock read (dioxus-utils `now_local_date_time`) needs a real
    // `Date`, so it is exercised in the browser, not here. These pin the
    // rendering half — the offset is applied, and it crosses midnight right.

    fn micros(utc: &str) -> i64 {
        DateTimeAsMicroseconds::parse_iso_string(utc)
            .unwrap()
            .unix_microseconds
    }

    #[test]
    fn an_instant_is_rendered_in_the_given_offset() {
        let utc = micros("2021-04-25T17:30:03.000000Z");

        assert_eq!(
            local_time_of_day(utc, TimeZone::from_minutes(60)),
            "18:30:03"
        );
        assert_eq!(
            local_date_time(utc, TimeZone::from_minutes(60)),
            "2021-04-25 18:30:03"
        );
    }

    #[test]
    fn a_negative_offset_can_roll_back_across_midnight() {
        // 00:30 UTC in UTC-2 is 22:30 the day before.
        let utc = micros("2021-04-25T00:30:00.000000Z");

        assert_eq!(
            local_date_time(utc, TimeZone::from_minutes(-120)),
            "2021-04-24 22:30:00"
        );
    }

    #[test]
    fn utc_is_the_no_op_offset() {
        let utc = micros("2021-04-25T17:30:03.000000Z");

        assert_eq!(local_time_of_day(utc, TimeZone::utc()), "17:30:03");
    }
}
