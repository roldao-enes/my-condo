//! pt-PT / Europe/Lisbon presentation helpers. Matches what `Intl` produced in the web app:
//! `1234,56 €`, `12 345,67 €` (NBSP grouping from 5 integer digits), `dd/MM/yyyy`.

use std::cmp::Ordering;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Europe::Lisbon;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

const NBSP: char = '\u{a0}';

/// Formats integer cents as a pt-PT euro amount.
pub fn money(cents: i64) -> String {
    let abs = cents.unsigned_abs();
    let euros = (abs / 100).to_string();
    let grouped = if euros.len() >= 5 {
        let first = euros.len() % 3;
        let mut out = String::new();
        if first > 0 {
            out.push_str(&euros[..first]);
        }
        for chunk in euros.as_bytes()[first..].chunks(3) {
            if !out.is_empty() {
                out.push(NBSP);
            }
            out.push_str(std::str::from_utf8(chunk).expect("ascii digits"));
        }
        out
    } else {
        euros
    };
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{grouped},{:02}{NBSP}€", abs % 100)
}

/// Cents as text for an input box: `-7,50`.
pub fn amount_input(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{},{:02}", abs / 100, abs % 100)
}

/// `dd/MM/yyyy`.
pub fn date(value: NaiveDate) -> String {
    value.format("%d/%m/%Y").to_string()
}

/// Reads a date as people type it: `dd/mm/aaaa` (also `d/m/aaaa`, `-` or `.` as separators) or
/// the ISO form `aaaa-mm-dd`. The year must have four digits and the day must exist.
pub fn parse_input_date(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    let parts: Vec<&str> = value.split(['/', '-', '.']).collect();
    let [a, b, c] = parts[..] else {
        return None;
    };
    let digits = |s: &str, min: usize, max: usize| {
        (min..=max).contains(&s.len()) && s.bytes().all(|byte| byte.is_ascii_digit())
    };
    let (year, month, day) = if digits(a, 4, 4) {
        (a, b, c) // aaaa-mm-dd
    } else {
        (c, b, a) // dd/mm/aaaa
    };
    if !digits(year, 4, 4) || !digits(month, 1, 2) || !digits(day, 1, 2) {
        return None;
    }
    NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

/// Up to two initials, uppercased: "maria silva santos" becomes "MS".
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// The calendar date in Lisbon at the given instant.
pub fn lisbon_date(now: DateTime<Utc>) -> NaiveDate {
    now.with_timezone(&Lisbon).date_naive()
}

/// An instant as the web app showed it (`dateStyle: short, timeStyle: short`, Lisbon time):
/// `17/11/2026, 14:30`.
pub fn date_time(instant: DateTime<Utc>) -> String {
    instant
        .with_timezone(&Lisbon)
        .format("%d/%m/%Y, %H:%M")
        .to_string()
}

/// Today's calendar date in Lisbon.
pub fn today() -> NaiveDate {
    lisbon_date(Utc::now())
}

/// Approximates `String.prototype.localeCompare(_, 'pt')` (ICU): base letters first
/// (whitespace < punctuation < digits < letters), then accents, then lowercase before uppercase.
pub fn collate_pt(a: &str, b: &str) -> Ordering {
    fn primary(s: &str) -> Vec<(u8, char)> {
        s.nfd()
            .filter(|c| !is_combining_mark(*c))
            .flat_map(char::to_lowercase)
            .map(|c| {
                let class = if c.is_whitespace() {
                    0
                } else if !c.is_alphanumeric() {
                    1
                } else if c.is_numeric() {
                    2
                } else {
                    3
                };
                (class, c)
            })
            .collect()
    }
    fn secondary(s: &str) -> String {
        s.nfd().flat_map(char::to_lowercase).collect()
    }
    fn tertiary(s: &str) -> Vec<bool> {
        s.chars().map(char::is_uppercase).collect()
    }
    primary(a)
        .cmp(&primary(b))
        .then_with(|| secondary(a).cmp(&secondary(b)))
        .then_with(|| tertiary(a).cmp(&tertiary(b)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn date_time_uses_lisbon_time_with_and_without_summer_time() {
        use chrono::TimeZone;
        let winter = chrono::Utc
            .with_ymd_and_hms(2026, 11, 17, 14, 30, 0)
            .unwrap();
        let summer = chrono::Utc.with_ymd_and_hms(2026, 7, 1, 23, 30, 0).unwrap();
        assert_eq!(super::date_time(winter), "17/11/2026, 14:30");
        assert_eq!(super::date_time(summer), "02/07/2026, 00:30");
    }

    use super::*;
    use chrono::TimeZone;

    // Expected strings were produced by Node's Intl.NumberFormat('pt-PT', EUR).
    #[test]
    fn money_matches_intl_pt_pt() {
        for (cents, expected) in [
            (0, "0,00\u{a0}€"),
            (5, "0,05\u{a0}€"),
            (-750, "-7,50\u{a0}€"),
            (99_999, "999,99\u{a0}€"),
            (123_456, "1234,56\u{a0}€"),
            (-123_456, "-1234,56\u{a0}€"),
            (1_234_567, "12\u{a0}345,67\u{a0}€"),
            (-100_000_000, "-1\u{a0}000\u{a0}000,00\u{a0}€"),
            (2_000_000_000, "20\u{a0}000\u{a0}000,00\u{a0}€"),
            (12_345_678_900, "123\u{a0}456\u{a0}789,00\u{a0}€"),
        ] {
            assert_eq!(money(cents), expected, "cents = {cents}");
        }
    }

    #[test]
    fn amount_input_and_date() {
        assert_eq!(amount_input(-750), "-7,50");
        assert_eq!(amount_input(-5), "-0,05");
        assert_eq!(amount_input(492_211), "4922,11");
        assert_eq!(
            date(NaiveDate::from_ymd_opt(2025, 2, 1).unwrap()),
            "01/02/2025"
        );
    }

    #[test]
    fn lisbon_date_follows_local_midnight_and_dst() {
        // Winter: UTC+0. Summer: UTC+1, so 23:30 UTC is already the next day in Lisbon.
        let winter = Utc.with_ymd_and_hms(2025, 1, 10, 23, 30, 0).unwrap();
        let summer = Utc.with_ymd_and_hms(2025, 7, 10, 23, 30, 0).unwrap();
        assert_eq!(
            lisbon_date(winter),
            NaiveDate::from_ymd_opt(2025, 1, 10).unwrap()
        );
        assert_eq!(
            lisbon_date(summer),
            NaiveDate::from_ymd_opt(2025, 7, 11).unwrap()
        );
    }

    #[test]
    fn parses_dates_as_people_type_them() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day);
        for (input, expected) in [
            ("05/03/2025", d(2025, 3, 5)),
            ("5/3/2025", d(2025, 3, 5)),
            (" 05-03-2025 ", d(2025, 3, 5)),
            ("05.03.2025", d(2025, 3, 5)),
            ("2025-03-05", d(2025, 3, 5)),
            ("29/02/2024", d(2024, 2, 29)),
        ] {
            assert_eq!(parse_input_date(input), expected, "{input:?}");
        }
        for bad in [
            "",
            "30/02/2025",
            "29/02/2025",
            "5/3/25",
            "2025/13/01",
            "aa/bb/cccc",
            "1/2",
            "1/2/3/4",
            "05/03/20255",
            "ontem",
        ] {
            assert_eq!(parse_input_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn initials_take_the_first_letters_of_the_first_two_words() {
        assert_eq!(initials("Maria Silva"), "MS");
        assert_eq!(initials("  maria   silva santos "), "MS");
        assert_eq!(initials("Ângela"), "Â");
        assert_eq!(initials("éva óscar"), "ÉÓ");
        assert_eq!(initials(""), "");
        assert_eq!(initials("   "), "");
    }

    // Expected order produced by Node's localeCompare(_, 'pt').
    #[test]
    fn collation_matches_icu_pt() {
        let expected = [
            "10A",
            "1A",
            "1B",
            "2A",
            "a b",
            "ab",
            "Alvaro",
            "Álvaro",
            "ana",
            "Ana",
            "Ângela",
            "Beatriz",
            "ca",
            "ça",
            "cb",
            "Maria  Silva",
            "Maria Silva",
            "Maria-Silva",
            "Z",
            "ze",
            "Zé",
        ];
        let mut shuffled: Vec<&str> = expected.iter().rev().copied().collect();
        shuffled.sort_by(|a, b| collate_pt(a, b));
        assert_eq!(shuffled, expected);
    }
}
