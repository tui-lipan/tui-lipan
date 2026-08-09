//! Date calculation utilities.

pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    let m = month as i32;
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + day as i32) % 7) as u32
}

pub fn prev_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

pub fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

/// Shift a calendar day by `delta` days, wrapping across month boundaries.
pub fn shift_day(year: i32, month: u32, day: u32, delta: i32) -> (i32, u32, u32) {
    let month = month.clamp(1, 12);
    let mut y = year;
    let mut m = month;
    let dim = days_in_month(y, m);
    let mut d = i64::from(day.clamp(1, dim)) + i64::from(delta);

    while d < 1 {
        let (py, pm) = prev_month(y, m);
        y = py;
        m = pm;
        d += i64::from(days_in_month(y, m));
    }
    loop {
        let dim = i64::from(days_in_month(y, m));
        if d <= dim {
            break;
        }
        d -= dim;
        let (ny, nm) = next_month(y, m);
        y = ny;
        m = nm;
    }

    (y, m, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_leap_year_applies_century_rules() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
    }

    #[test]
    fn is_leap_year_handles_regular_years() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn days_in_month_handles_february_for_leap_and_common_years() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
    }

    #[test]
    fn days_in_month_handles_30_and_31_day_months() {
        assert_eq!(days_in_month(2024, 1), 31);
        assert_eq!(days_in_month(2024, 4), 30);
    }

    #[test]
    fn weekday_matches_known_calendar_anchors() {
        assert_eq!(weekday(1970, 1, 1), 4);
        assert_eq!(weekday(2000, 1, 1), 6);
        assert_eq!(weekday(2024, 1, 1), 1);
    }

    #[test]
    fn prev_month_handles_year_rollover_and_regular_case() {
        assert_eq!(prev_month(2024, 1), (2023, 12));
        assert_eq!(prev_month(2024, 5), (2024, 4));
    }

    #[test]
    fn next_month_handles_year_rollover_and_regular_case() {
        assert_eq!(next_month(2024, 12), (2025, 1));
        assert_eq!(next_month(2024, 5), (2024, 6));
    }

    #[test]
    fn shift_day_moves_within_and_across_months() {
        assert_eq!(shift_day(2024, 1, 15, 1), (2024, 1, 16));
        assert_eq!(shift_day(2024, 1, 15, -1), (2024, 1, 14));
        assert_eq!(shift_day(2024, 1, 31, 1), (2024, 2, 1));
        assert_eq!(shift_day(2024, 3, 1, -1), (2024, 2, 29));
        assert_eq!(shift_day(2024, 1, 10, -7), (2024, 1, 3));
        assert_eq!(shift_day(2024, 1, 3, -7), (2023, 12, 27));
    }
}
