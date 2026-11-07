//! When a task was said, and the day and time it names: the time zone, the
//! Persian (Jalali) calendar, and turning the model's date and time into real
//! ones.
//!
//! Models are bad at date arithmetic, and worse with the Jalali calendar, so
//! the prompt gets a table of the coming days in both calendars to copy from,
//! and any other Jalali date comes back as Jalali and is converted here.

use jiff::civil::{self, Date, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize};

/// When the words were spoken, and in which time zone. Dates in the task are
/// relative to this, not to when the task reaches Google.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Said {
    /// Milliseconds since 1970 (UTC).
    pub at_ms: i64,
    /// An IANA name like "Europe/Berlin"; empty for the system's own.
    #[serde(default)]
    pub tz: String,
}

impl Said {
    /// Now, in this computer's time zone.
    pub fn now() -> Said {
        Said::at(Timestamp::now().as_millisecond(), "")
    }

    /// `tz` empty means the system's time zone; its name is kept when there
    /// is one, so a retry after a trip still uses the zone it was said in.
    pub fn at(at_ms: i64, tz: &str) -> Said {
        let tz = match tz.trim() {
            "" => TimeZone::system().iana_name().unwrap_or_default().to_owned(),
            t => t.to_owned(),
        };
        Said { at_ms, tz }
    }

    pub fn time_zone(&self) -> TimeZone {
        match self.tz.trim() {
            "" => TimeZone::system(),
            name => TimeZone::get(name).unwrap_or_else(|_| TimeZone::system()),
        }
    }

    pub fn zoned(&self) -> Zoned {
        let ts = Timestamp::from_millisecond(self.at_ms).unwrap_or_else(|_| Timestamp::now());
        ts.to_zoned(self.time_zone())
    }
}

// ---- Jalali calendar ----
//
// The arithmetic calendar of jalaali-js (Borkowski's algorithm with the
// table of breaks), which matches the official calendar for 1178-1633 AP.

const BREAKS: [i64; 20] =
    [-61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324, 2394, 2456, 3178];

/// A day in the Persian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jalali {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

/// For Jalali year `jy`: years since the last leap year (0 = leap), and the
/// March day of Nowruz in Gregorian year jy + 621. None outside the table.
fn jal_cal(jy: i64) -> Option<(i64, i64)> {
    if jy < BREAKS[0] || jy >= BREAKS[BREAKS.len() - 1] {
        return None;
    }
    let gy = jy + 621;
    let mut leap_j = -14;
    let mut jp = BREAKS[0];
    let mut jump = 0;
    for &jm in &BREAKS[1..] {
        jump = jm - jp;
        if jy < jm {
            break;
        }
        leap_j += jump / 33 * 8 + (jump % 33) / 4;
        jp = jm;
    }
    let mut n = jy - jp;
    leap_j += n / 33 * 8 + (n % 33 + 3) / 4;
    if jump % 33 == 4 && jump - n == 4 {
        leap_j += 1;
    }
    let leap_g = gy / 4 - (gy / 100 + 1) * 3 / 4 - 150;
    let march = 20 + leap_j - leap_g;
    if jump - n < 6 {
        n = n - jump + (jump + 4) / 33 * 33;
    }
    let mut leap = ((n + 1) % 33 - 1) % 4;
    if leap == -1 {
        leap = 4;
    }
    Some((leap, march))
}

/// Julian day number of a Gregorian date.
fn g2d(gy: i64, gm: i64, gd: i64) -> i64 {
    let d = (gy + (gm - 8) / 6 + 100100) * 1461 / 4 + (153 * ((gm + 9) % 12) + 2) / 5 + gd - 34840408;
    d - (gy + 100100 + (gm - 8) / 6) / 100 * 3 / 4 + 752
}

/// Gregorian date of a Julian day number.
fn d2g(jdn: i64) -> (i64, i64, i64) {
    let mut j = 4 * jdn + 139361631;
    j += (4 * jdn + 183187720) / 146097 * 3 / 4 * 4 - 3908;
    let i = (j % 1461) / 4 * 5 + 308;
    let gd = (i % 153) / 5 + 1;
    let gm = (i / 153) % 12 + 1;
    let gy = j / 1461 - 100100 + (8 - gm) / 6;
    (gy, gm, gd)
}

fn jdn(d: Date) -> i64 {
    g2d(d.year().into(), d.month().into(), d.day().into())
}

fn from_jdn(n: i64) -> Option<Date> {
    let (y, m, d) = d2g(n);
    Date::new(i16::try_from(y).ok()?, m as i8, d as i8).ok()
}

/// `d` plus `days` (which may be negative).
pub fn add_days(d: Date, days: i64) -> Date {
    from_jdn(jdn(d) + days).unwrap_or(d)
}

/// Days from `a` to `b`.
pub fn days_between(a: Date, b: Date) -> i64 {
    jdn(b) - jdn(a)
}

pub fn is_jalali_leap(year: i64) -> bool {
    jal_cal(year).is_some_and(|(leap, _)| leap == 0)
}

pub fn jalali_month_len(year: i64, month: i64) -> i64 {
    match month {
        1..=6 => 31,
        7..=11 => 30,
        12 if is_jalali_leap(year) => 30,
        _ => 29,
    }
}

pub fn to_jalali(d: Date) -> Jalali {
    let n = jdn(d);
    let mut jy = i64::from(d.year()) - 621;
    let Some((leap, march)) = jal_cal(jy) else {
        return Jalali { year: jy, month: 1, day: 1 };
    };
    let mut k = n - g2d(jy + 621, 3, march);
    if k >= 0 {
        if k <= 185 {
            return Jalali { year: jy, month: 1 + k / 31, day: k % 31 + 1 };
        }
        k -= 186;
    } else {
        jy -= 1;
        k += 179;
        if leap == 1 {
            k += 1;
        }
    }
    Jalali { year: jy, month: 7 + k / 30, day: k % 30 + 1 }
}

/// The Gregorian date of a Jalali one; None if there's no such day.
pub fn from_jalali(year: i64, month: i64, day: i64) -> Option<Date> {
    if !(1..=12).contains(&month) || day < 1 || day > jalali_month_len(year, month) {
        return None;
    }
    let (_, march) = jal_cal(year)?;
    from_jdn(g2d(year + 621, 3, march) + (month - 1) * 31 - month / 7 * (month - 7) + day - 1)
}

pub const JALALI_MONTHS: [&str; 12] =
    ["فروردین", "اردیبهشت", "خرداد", "تیر", "مرداد", "شهریور", "مهر", "آبان", "آذر", "دی", "بهمن", "اسفند"];

fn persian_weekday(w: Weekday) -> &'static str {
    match w {
        Weekday::Saturday => "شنبه",
        Weekday::Sunday => "یکشنبه",
        Weekday::Monday => "دوشنبه",
        Weekday::Tuesday => "سه‌شنبه",
        Weekday::Wednesday => "چهارشنبه",
        Weekday::Thursday => "پنج‌شنبه",
        Weekday::Friday => "جمعه",
    }
}

fn english_weekday(w: Weekday) -> &'static str {
    match w {
        Weekday::Monday => "Monday",
        Weekday::Tuesday => "Tuesday",
        Weekday::Wednesday => "Wednesday",
        Weekday::Thursday => "Thursday",
        Weekday::Friday => "Friday",
        Weekday::Saturday => "Saturday",
        Weekday::Sunday => "Sunday",
    }
}

/// "Thursday 2026-11-12 (پنج‌شنبه ۲۱ آبان ۱۴۰۵)", for the prompt.
fn both(d: Date) -> String {
    let j = to_jalali(d);
    format!(
        "{} {d} = {} {} {} {} ({:04}-{:02}-{:02})",
        english_weekday(d.weekday()),
        persian_weekday(d.weekday()),
        j.day,
        JALALI_MONTHS[(j.month - 1) as usize],
        j.year,
        j.year,
        j.month,
        j.day
    )
}

/// How many coming days the prompt lists.
const TABLE_DAYS: i64 = 15;

/// What the model needs to know about now, for the prompt: the time, the
/// time zone, and the coming days in both calendars.
pub fn context(now: &Zoned) -> String {
    let today = now.date();
    let zone = now.time_zone().iana_name().map(|n| format!("{n}, ")).unwrap_or_default();
    let mut out = format!(
        "Now: {} {}, time zone {zone}UTC{}.\nToday in the Persian calendar: {}.\n{}\n\nThe coming days (Gregorian, then Persian):\n",
        english_weekday(today.weekday()),
        now.strftime("%Y-%m-%d %H:%M"),
        now.strftime("%:z"),
        {
            let j = to_jalali(today);
            format!(
                "{} {} {} {}",
                persian_weekday(today.weekday()),
                j.day,
                JALALI_MONTHS[(j.month - 1) as usize],
                j.year
            )
        },
        {
            let j = to_jalali(today);
            format!(
                "This Gregorian month has {} days; this Persian month ({}) has {}. Persian weeks start on Saturday.",
                today.days_in_month(),
                JALALI_MONTHS[(j.month - 1) as usize],
                jalali_month_len(j.year, j.month)
            )
        },
    );
    for i in 0..TABLE_DAYS {
        let d = add_days(today, i);
        let label = match i {
            0 => " (today)",
            1 => " (tomorrow)",
            2 => " (day after tomorrow)",
            _ => "",
        };
        out.push_str(&format!("{}{label}\n", both(d)));
    }
    out
}

// ---- Reading the model's date and time ----

/// Which calendar a date the model gave is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Calendar {
    #[default]
    Gregorian,
    Jalali,
}

/// The day (and maybe time) a task is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct When {
    pub date: Option<Date>,
    pub time: Option<Time>,
}

/// Persian and Arabic digits as ASCII, other separators as '-'.
fn ascii_digits(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            '۰'..='۹' => char::from(b'0' + (c as u32 - '۰' as u32) as u8),
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            '/' | '.' => '-',
            c => c,
        })
        .collect()
}

fn numbers(s: &str, sep: char) -> Option<Vec<i64>> {
    s.split(sep).map(|p| p.trim().parse::<i64>().ok()).collect()
}

/// "HH:MM" (or "H", "H:MM", "HH:MM:SS") as a time of day.
pub fn parse_time(s: &str) -> Option<Time> {
    let s = ascii_digits(s);
    if s.is_empty() {
        return None;
    }
    let n = numbers(&s, ':')?;
    let (h, m) = match n[..] {
        [h] => (h, 0),
        [h, m] | [h, m, _] => (h, m),
        _ => return None,
    };
    Time::new(i8::try_from(h).ok()?, i8::try_from(m).ok()?, 0, 0).ok()
}

/// Turns the model's `date` ("YYYY-MM-DD", "MM-DD" or empty) in `calendar`
/// and `time` ("HH:MM" or empty) into a day and time, relative to `now`.
/// Anything Mirza had to assume or drop is added to `notes`, in plain words.
pub fn resolve(date: &str, calendar: Calendar, time: &str, now: &Zoned, notes: &mut Vec<String>) -> When {
    let today = now.date();
    let date_s = ascii_digits(date);
    let mut time_v = parse_time(time);
    if !time.trim().is_empty() && time_v.is_none() {
        notes.push(format!("couldn't read the time {:?}, so the task has none", time.trim()));
    }
    let mut date_v = None;
    if !date_s.is_empty() {
        date_v = read_date(&date_s, calendar, today);
        if date_v.is_none() {
            notes.push(format!("couldn't read the date {:?}, so the task has none", date.trim()));
        }
    }
    if let Some(d) = date_v {
        let ahead = days_between(today, d);
        if !(-366..=5 * 366).contains(&ahead) {
            notes.push(format!("{d} is too far off to be meant, so the task has no date"));
            date_v = None;
            time_v = None;
        } else if ahead < 0 {
            notes.push(format!("{d} has already passed"));
        } else if ahead == 0 && time_v.is_some_and(|t| t < now.time()) {
            notes.push("that time has already passed today".into());
        }
    } else if let Some(t) = time_v {
        // A time without a day: the next time the clock shows it.
        if t > now.time() {
            date_v = Some(today);
        } else {
            date_v = Some(add_days(today, 1));
            notes.push("no day was said and that time has passed today, so it's for tomorrow".into());
        }
    }
    When { date: date_v, time: time_v.filter(|_| date_v.is_some()) }
}

fn read_date(s: &str, calendar: Calendar, today: Date) -> Option<Date> {
    let n = numbers(s, '-')?;
    let (year, month, day) = match n[..] {
        [y, m, d] => (Some(y), m, d),
        [m, d] => (None, m, d),
        _ => return None,
    };
    // The year shows which calendar it really is, whatever the model said.
    let calendar = match year {
        Some(y) if (1300..1500).contains(&y) => Calendar::Jalali,
        Some(y) if (1900..2200).contains(&y) => Calendar::Gregorian,
        _ => calendar,
    };
    match (calendar, year) {
        (Calendar::Gregorian, Some(y)) => Date::new(i16::try_from(y).ok()?, month as i8, day as i8).ok(),
        (Calendar::Jalali, Some(y)) => from_jalali(y, month, day),
        (Calendar::Gregorian, None) => {
            let y = i64::from(today.year());
            [y, y + 1, y + 4]
                .iter()
                .filter_map(|&y| Date::new(y as i16, month as i8, day as i8).ok())
                .find(|d| *d >= today)
        }
        (Calendar::Jalali, None) => {
            let y = to_jalali(today).year;
            (y..y + 5).filter_map(|y| from_jalali(y, month, day)).find(|d| *d >= today)
        }
    }
}

/// The date as Google Tasks wants it: midnight UTC of that day (Tasks keeps
/// only the date).
pub fn task_due(d: Date) -> String {
    format!("{d}T00:00:00.000Z")
}

/// Start and end of an event as Calendar wants them: the local time with its
/// offset, plus the IANA `timeZone` when the zone has a name.
pub fn event_times(d: Date, t: Time, minutes: i64, tz: &TimeZone) -> Option<(serde_json::Value, serde_json::Value)> {
    let start = d.to_datetime(t).to_zoned(tz.clone()).ok()?;
    let end = start.checked_add(jiff::SignedDuration::from_mins(minutes.max(1))).ok()?;
    let one = |z: &Zoned| {
        let mut v = serde_json::json!({ "dateTime": z.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string() });
        if let Some(name) = tz.iana_name() {
            v["timeZone"] = name.into();
        }
        v
    };
    Some((one(&start), one(&end)))
}

/// "Thu 12 Nov" or "Thu 12 Nov 2027", plus " 15:00", for notifications.
pub fn describe(when: &When, today: Date) -> String {
    let Some(d) = when.date else { return String::new() };
    let mut out = match days_between(today, d) {
        0 => "today".to_owned(),
        1 => "tomorrow".to_owned(),
        _ if d.year() == today.year() || (0..300).contains(&days_between(today, d)) => {
            d.strftime("%a %-d %b").to_string()
        }
        _ => d.strftime("%a %-d %b %Y").to_string(),
    };
    if let Some(t) = when.time {
        out.push_str(&format!(" {}", t.strftime("%H:%M")));
    }
    out
}

/// A Gregorian date, or None for a bad one (for dates typed in the edit form).
pub fn parse_date(s: &str) -> Option<Date> {
    let n = numbers(&ascii_digits(s), '-')?;
    match n[..] {
        [y, m, d] => civil::Date::new(i16::try_from(y).ok()?, i8::try_from(m).ok()?, i8::try_from(d).ok()?).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn now(s: &str) -> Zoned {
        format!("{s}[Europe/Berlin]").parse().unwrap()
    }

    #[test]
    fn jalali_known_days() {
        // Pairs from jalaali-js's tests and Nowruz of recent years.
        let pairs = [
            (date(1981, 8, 17), (1360, 5, 26)),
            (date(2013, 1, 10), (1391, 10, 21)),
            (date(2014, 8, 4), (1393, 5, 13)),
            (date(2024, 3, 20), (1403, 1, 1)),
            (date(2025, 3, 20), (1403, 12, 30)),
            (date(2025, 3, 21), (1404, 1, 1)),
            (date(2026, 11, 6), (1405, 8, 15)),
        ];
        for (g, (y, m, d)) in pairs {
            assert_eq!(to_jalali(g), Jalali { year: y, month: m, day: d }, "{g}");
            assert_eq!(from_jalali(y, m, d), Some(g), "{y}-{m}-{d}");
        }
    }

    #[test]
    fn jalali_leap_years_and_month_lengths() {
        assert!(!is_jalali_leap(1393));
        assert!(!is_jalali_leap(1394));
        assert!(is_jalali_leap(1395));
        assert!(is_jalali_leap(1403));
        assert_eq!(jalali_month_len(1393, 12), 29);
        assert_eq!(jalali_month_len(1395, 12), 30);
        assert_eq!(from_jalali(1404, 12, 30), None);
        assert_eq!(from_jalali(1405, 13, 1), None);
    }

    #[test]
    fn jalali_round_trip() {
        let mut d = date(1921, 3, 21);
        while d < date(2120, 3, 21) {
            let j = to_jalali(d);
            assert_eq!(from_jalali(j.year, j.month, j.day), Some(d), "{d} -> {j:?}");
            d = add_days(d, 1);
        }
    }

    #[test]
    fn context_lists_the_coming_days_in_both_calendars() {
        let c = context(&now("2026-11-06T18:40"));
        assert!(c.contains("Now: Friday 2026-11-06 18:40, time zone Europe/Berlin, UTC+01:00."), "{c}");
        assert!(c.contains("Today in the Persian calendar: جمعه 15 آبان 1405."), "{c}");
        assert!(c.contains("Saturday 2026-11-07 = شنبه 16 آبان 1405 (1405-08-16) (tomorrow)"), "{c}");
        assert_eq!(c.lines().filter(|l| l.contains(" = ")).count(), TABLE_DAYS as usize);
    }

    #[test]
    fn dates_resolve() {
        let n = now("2026-11-06T18:40");
        let mut notes = vec![];
        let r = |d: &str, c, t: &str, notes: &mut Vec<String>| resolve(d, c, t, &n, notes);

        assert_eq!(r("", Calendar::Gregorian, "", &mut notes), When::default());
        assert_eq!(
            r("2026-11-07", Calendar::Gregorian, "15:00", &mut notes),
            When { date: Some(date(2026, 11, 7)), time: Some(civil::time(15, 0, 0, 0)) }
        );
        // No year: the next time that day comes.
        assert_eq!(r("01-05", Calendar::Gregorian, "", &mut notes).date, Some(date(2027, 1, 5)));
        assert_eq!(r("11-06", Calendar::Gregorian, "", &mut notes).date, Some(date(2026, 11, 6)));
        assert_eq!(r("08-20", Calendar::Jalali, "", &mut notes).date, Some(date(2026, 11, 11)));
        assert_eq!(r("01-01", Calendar::Jalali, "", &mut notes).date, Some(date(2027, 3, 21)));
        assert_eq!(r("۱۴۰۵/۰۹/۰۱", Calendar::Gregorian, "", &mut notes).date, Some(date(2026, 11, 22)));
        assert!(notes.is_empty(), "{notes:?}");

        // A time with no day: today if it's still ahead, else tomorrow.
        assert_eq!(r("", Calendar::Gregorian, "20:00", &mut notes).date, Some(date(2026, 11, 6)));
        assert_eq!(r("", Calendar::Gregorian, "۹:۳۰", &mut notes).date, Some(date(2026, 11, 7)));
        assert_eq!(notes.len(), 1);

        notes.clear();
        let w = r("2026-11-01", Calendar::Gregorian, "", &mut notes);
        assert_eq!(w.date, Some(date(2026, 11, 1)));
        assert!(notes[0].contains("already passed"));

        notes.clear();
        assert_eq!(r("2019-01-01", Calendar::Gregorian, "10:00", &mut notes), When::default());
        assert_eq!(r("tomorrow", Calendar::Gregorian, "25:00", &mut notes), When::default());
        assert_eq!(notes.len(), 3, "{notes:?}");
    }

    #[test]
    fn yearless_persian_dates_around_nowruz() {
        let mut notes = vec![];
        // 1403 is a leap year: 30 Esfand 1403 is 2025-03-20.
        let n: Zoned = "2025-03-19T10:00[Asia/Tehran]".parse().unwrap();
        assert_eq!(resolve("12-30", Calendar::Jalali, "", &n, &mut notes).date, Some(date(2025, 3, 20)));
        assert_eq!(resolve("01-02", Calendar::Jalali, "", &n, &mut notes).date, Some(date(2025, 3, 22)));
        assert_eq!(resolve("12-28", Calendar::Jalali, "", &n, &mut notes).date, Some(date(2026, 3, 19)));
        // 1404 isn't a leap year, so the next 30 Esfand is 1408's.
        let n: Zoned = "2026-03-01T10:00[Asia/Tehran]".parse().unwrap();
        assert_eq!(resolve("12-30", Calendar::Jalali, "", &n, &mut notes).date, from_jalali(1408, 12, 30));
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn calendar_wants_local_time_and_zone() {
        let tz = TimeZone::get("Europe/Berlin").unwrap();
        let (s, e) = event_times(date(2027, 3, 28), civil::time(1, 30, 0, 0), 60, &tz).unwrap();
        assert_eq!(s, serde_json::json!({"dateTime": "2027-03-28T01:30:00+01:00", "timeZone": "Europe/Berlin"}));
        // The clocks go forward at 02:00 that night.
        assert_eq!(e["dateTime"], "2027-03-28T03:30:00+02:00");
        let fixed = TimeZone::fixed(jiff::tz::offset(3));
        let (s, _) = event_times(date(2026, 11, 7), civil::time(9, 0, 0, 0), 30, &fixed).unwrap();
        assert_eq!(s, serde_json::json!({"dateTime": "2026-11-07T09:00:00+03:00"}));
        assert_eq!(task_due(date(2026, 11, 7)), "2026-11-07T00:00:00.000Z");
    }

    #[test]
    fn describes_days_for_people() {
        let today = date(2026, 11, 6);
        let w = |d, t| When { date: Some(d), time: t };
        assert_eq!(describe(&w(today, None), today), "today");
        assert_eq!(describe(&w(date(2026, 11, 7), Some(civil::time(15, 0, 0, 0))), today), "tomorrow 15:00");
        assert_eq!(describe(&w(date(2026, 11, 12), None), today), "Thu 12 Nov");
        assert_eq!(describe(&w(date(2028, 1, 3), None), today), "Mon 3 Jan 2028");
    }
}
