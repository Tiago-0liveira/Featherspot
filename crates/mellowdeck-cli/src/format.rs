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
    format!("{:>5}", value)
}

/// Keeps Spotify's typed release data presentation-free while accepting only valid precisions.
pub fn release_label(value: Option<&str>, precision: Option<&str>) -> Option<String> {
    match (value, precision) {
        (Some(year), Some("year")) if year.len() == 4 && year.chars().all(char::is_numeric) => {
            Some(year.into())
        }
        (Some(date), Some("day")) if date.len() >= 7 => Some(date[..7].replace('-', " ")),
        _ => None,
    }
}
