//! The wall clock of this Mac, as JS `Date`'s local getters and `new Date(y, m, d, h, min)` have it: a time of day that
//! follows the clock on the wall through a change to or from summer time. The zone comes from `host::utc_offset`.

use crate::host;

const DAY_MS: i64 = 86_400_000;

#[cfg(not(target_arch = "wasm32"))]
pub mod testing {
    use std::{cell::RefCell, rc::Rc};

    thread_local! {
        pub(super) static OFFSET: RefCell<Option<Rc<dyn Fn(f64) -> i32>>> = const { RefCell::new(None) };
    }

    /// From now on, on this thread, the zone is `offset`: minutes ahead of UTC at an instant in ms.
    pub fn zone(offset: impl Fn(f64) -> i32 + 'static) {
        OFFSET.with(|slot| *slot.borrow_mut() = Some(Rc::new(offset)));
    }
}

fn offset(ms: i64) -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(zone) = testing::OFFSET.with(|slot| slot.borrow().clone()) {
        return i64::from(zone(ms as f64));
    }
    i64::from(host::utc_offset(ms as f64))
}

/// A moment as the wall clock shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Local {
    pub year: i64,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    /// 0 for Sunday, as `Date.getDay()`.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Days since 1970-01-01 of a civil date (Hinnant's algorithm); `day` may run past the month.
fn days_from_civil(year: i64, month: u32, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400));
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 + (day - 1)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    (yoe + era * 400 + i64::from(m <= 2), m as u32, d as u32)
}

/// What the wall clock shows at `ms`.
pub fn local(ms: i64) -> Local {
    let wall = ms + offset(ms) * 60_000;
    let days = wall.div_euclid(DAY_MS);
    let minutes = wall.rem_euclid(DAY_MS) / 60_000;
    let (year, month, day) = civil_from_days(days);
    Local { year, month, day, weekday: (days + 4).rem_euclid(7) as u32, hour: (minutes / 60) as u32, minute: (minutes % 60) as u32 }
}

/// The instant the wall clock first shows `hour`:`minute` on that date, as `new Date(year, month - 1, day, hour,
/// minute)`. A `day` past the end of the month counts on into the next. A time that doesn't exist, the hour skipped
/// when the clock goes forward, is the same instant a clock an hour behind would give.
pub fn at(year: i64, month: u32, day: i64, hour: u32, minute: u32) -> i64 {
    let naive = days_from_civil(year, month, day) * DAY_MS + (i64::from(hour) * 60 + i64::from(minute)) * 60_000;
    let before = offset(naive - DAY_MS);
    let first = naive - before * 60_000;
    let now = offset(first);
    if now == before {
        return first;
    }
    let second = naive - now * 60_000;
    if offset(second) == now { second } else { first }
}

/// The midnight that starts the day of `ms`.
pub fn start_of_day(ms: i64) -> i64 {
    let l = local(ms);
    at(l.year, l.month, i64::from(l.day), 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zagreb in 2026: one hour ahead of UTC, two from 01:00 UTC on March 29 to 01:00 UTC on October 25.
    fn zagreb(ms: f64) -> i32 {
        let (start, end) = (at_utc(2026, 3, 29), at_utc(2026, 10, 25));
        if (start..end).contains(&(ms as i64)) { 120 } else { 60 }
    }

    fn at_utc(year: i64, month: u32, day: i64) -> i64 {
        days_from_civil(year, month, day) * DAY_MS + 3_600_000
    }

    #[test]
    fn the_wall_clock_follows_the_zone_through_summer_time() {
        testing::zone(zagreb);
        assert_eq!(local(at_utc(2026, 5, 4)), Local { year: 2026, month: 5, day: 4, weekday: 1, hour: 3, minute: 0 });
        // 09:00 on both sides of the change is 08:00 and 07:00 UTC.
        assert_eq!(at(2026, 3, 28, 9, 0), at_utc(2026, 3, 28) + 7 * 3_600_000);
        assert_eq!(at(2026, 3, 29, 9, 0), at_utc(2026, 3, 29) + 6 * 3_600_000);
        assert_eq!(at(2026, 3, 29, 9, 0) - at(2026, 3, 28, 9, 0), 23 * 3_600_000);
        // 02:30 on the 29th doesn't exist: it is 03:30.
        assert_eq!(local(at(2026, 3, 29, 2, 30)), Local { year: 2026, month: 3, day: 29, weekday: 0, hour: 3, minute: 30 });
        // Days past the month's end roll over, and the hour that happens twice is its first.
        assert_eq!(at(2026, 3, 32, 9, 0), at(2026, 4, 1, 9, 0));
        assert_eq!(at(2026, 10, 25, 2, 30), at_utc(2026, 10, 25) - 30 * 60_000);
        testing::zone(|_| 0);
        assert_eq!(start_of_day(at_utc(2026, 5, 4) + 5 * 3_600_000), at_utc(2026, 5, 4) - 3_600_000);
    }
}
