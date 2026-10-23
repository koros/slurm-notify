//! Shared strict scheduler UTC timestamp interpretation.
use time::{Date, Month, PrimitiveDateTime, Time};
pub(super) fn valid(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return false;
    }
    if b.iter()
        .enumerate()
        .any(|(i, c)| ![4, 7, 10, 13, 16].contains(&i) && !c.is_ascii_digit())
    {
        return false;
    }
    let number = |start: usize, end: usize| value[start..end].parse::<u32>().unwrap();
    let year = number(0, 4);
    let month = number(5, 7);
    let day = number(8, 10);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    year > 0
        && day > 0
        && day <= days
        && number(11, 13) < 24
        && number(14, 16) < 60
        && number(17, 19) < 60
}
pub(super) fn parse(value: &str) -> Result<i64, ()> {
    if !valid(value) {
        return Err(());
    }
    let number = |start: usize, end: usize| value[start..end].parse::<u32>().map_err(|_| ());
    let date = Date::from_calendar_date(
        number(0, 4)? as i32,
        Month::try_from(number(5, 7)? as u8).map_err(|_| ())?,
        number(8, 10)? as u8,
    )
    .map_err(|_| ())?;
    let time = Time::from_hms(
        number(11, 13)? as u8,
        number(14, 16)? as u8,
        number(17, 19)? as u8,
    )
    .map_err(|_| ())?;
    let seconds = PrimitiveDateTime::new(date, time)
        .assume_utc()
        .unix_timestamp();
    if seconds < 0 {
        return Err(());
    }
    Ok(seconds)
}
