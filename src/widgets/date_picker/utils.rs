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
}
