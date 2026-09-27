//! The one place the application reads the clock.
//!
//! The model and the layout carry a plain [`Stamp`] — a date as the wall
//! shows it — and never read the time themselves, so a layout is the same
//! whenever it runs and no test depends on today. This turns "now" into one,
//! in the person's own time zone, which is what a footer's date means.

use tessera_document::variables::Stamp;

/// Now, on the person's own calendar and clock.
pub fn now() -> Stamp {
    stamp_of(&jiff::Zoned::now())
}

fn stamp_of(zoned: &jiff::Zoned) -> Stamp {
    // jiff keeps each field in its own range — month 1 to 12, hour 0 to 23 —
    // so these narrowings cannot fail; zero is the answer if one ever did.
    let byte = |v: i8| u8::try_from(v).unwrap_or(0);
    Stamp {
        year: i32::from(zoned.year()),
        month: byte(zoned.month()),
        day: byte(zoned.day()),
        hour: byte(zoned.hour()),
        minute: byte(zoned.minute()),
        second: byte(zoned.second()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_keeps_its_wall_clock_in_its_own_zone() {
        // 23:30 in São Paulo is already tomorrow in UTC: the footer must
        // say the day the person is living in.
        let zoned: jiff::Zoned = "2026-09-27T23:30:05-03:00[America/Sao_Paulo]"
            .parse()
            .expect("a zoned time");
        let stamp = stamp_of(&zoned);
        assert_eq!(stamp.iso(), "2026-09-27T23:30:05");
    }

    #[test]
    fn now_is_a_real_date() {
        let now = now();
        assert!(now.year >= 2026 && (1..=12).contains(&now.month), "{now:?}");
    }
}
