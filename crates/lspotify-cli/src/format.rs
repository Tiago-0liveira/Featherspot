/// Formats a count into the fixed-width field used by artist headers.
pub fn compact_count(value: u64) -> String {
    const UNITS: [(u64, char); 3] = [(1_000_000_000, 'B'), (1_000_000, 'M'), (1_000, 'K')];
    for (divisor, suffix) in UNITS {
        if value >= divisor {
            let tenths = value.saturating_mul(10).saturating_add(divisor / 2) / divisor;
            if tenths >= 10_000 && suffix != 'B' {
                continue;
            }
            return format!("{:>5}", format!("{}.{:01}{suffix}", tenths / 10, tenths % 10));
        }
    }
    format!("{value:>5}")
}

/// Keeps Spotify's typed release data presentation-free while accepting only valid precisions.
pub fn release_label(value: Option<&str>, precision: Option<&str>) -> Option<String> {
    match (value, precision) {
        (Some(year), Some("year")) if year.len() == 4 && year.chars().all(char::is_numeric) => {
            Some(year.into())
        }
        (Some(date), Some("day")) if date.len() >= 7 && date.is_char_boundary(7) => {
            Some(date[..7].replace('-', " "))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_label_valid_day() {
        assert_eq!(release_label(Some("2023-05-12"), Some("day")), Some("2023 05".into()));
        assert_eq!(release_label(Some("2023-05"), Some("day")), Some("2023 05".into()));
    }

    #[test]
    fn test_release_label_utf8_char_boundary() {
        // "2023-日-12": '2','0','2','3','-' is 5 bytes. '日' is 3 bytes (indices 5, 6, 7).
        // Slicing at index 7 falls in the middle of '日', which would panic without char boundary check.
        assert_eq!(release_label(Some("2023-日-12"), Some("day")), None);

        // Multibyte character where byte 7 is a valid char boundary:
        // '年' is 3 bytes (0..3), '-' is 1 byte (3..4), '202' is 3 bytes (4..7).
        assert_eq!(release_label(Some("年-2023"), Some("day")), Some("年 202".into()));
    }

    #[test]
    fn test_release_label_short_strings() {
        assert_eq!(release_label(Some("2023-0"), Some("day")), None);
        assert_eq!(release_label(Some("2023"), Some("day")), None);
        assert_eq!(release_label(Some(""), Some("day")), None);
    }

    #[test]
    fn test_release_label_none_and_invalid_cases() {
        assert_eq!(release_label(None, Some("day")), None);
        assert_eq!(release_label(Some("2023-05-12"), None), None);
        assert_eq!(release_label(None, None), None);
        assert_eq!(release_label(Some(""), None), None);
        assert_eq!(release_label(Some(""), Some("year")), None);
        assert_eq!(release_label(Some("2023"), Some("month")), None);
    }

    #[test]
    fn test_release_label_year() {
        assert_eq!(release_label(Some("2023"), Some("year")), Some("2023".into()));
        assert_eq!(release_label(Some("202a"), Some("year")), None);
        assert_eq!(release_label(Some("202"), Some("year")), None);
        assert_eq!(release_label(Some("20234"), Some("year")), None);
    }
}
