use clap::Parser;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "workdup", about = "Temporal workflow history deduplication")]
pub struct Cli {
    /// Temporal namespace (falls back to env var)
    #[arg(long, env = "TEMPORAL_NAMESPACE")]
    pub namespace: Option<String>,

    /// Cleanup interval, e.g. `23s`, `30m`, `24h`, `4d` (units: s, m, h, d — d is max)
    #[arg(long, value_parser = parse_cleanup_interval, default_value= "1d")]
    pub cleanup_interval: Duration,

    /// Scaning running workflow interval, e.g. `23s`, `30m`, `24h`, `4d` (units: s, m, h, d — d is max)
    #[arg(long, value_parser = parse_cleanup_interval, default_value="1h")]
    pub scan_interval: Duration,
}

fn parse_cleanup_interval(raw: &str) -> Result<Duration, String> {
    let s = raw.trim();

    let idx_of_first_letter = s.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| {
        format!("'{raw}': missing unit - expected number + s|m|h|d (e.g. 23s, 24h, 4d)")
    })?;

    let (num_unit, time_unit) = s.split_at(idx_of_first_letter);

    if num_unit.is_empty() {
        return Err(format!("'{raw}': missing number before unit '{time_unit}'"));
    }

    let number: u64 = num_unit
        .parse()
        .map_err(|_| format!("'{raw}': invalid number '{num_unit}'"))?;

    if number == 0 {
        return Err(format!("'{raw}': interval must be greater than zero"));
    }

    let in_secs = match time_unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        other => {
            return Err(format!(
                "'{raw}': unknown unit '{other}' — allowed: s, m, h, d (d is the maximum)"
            ));
        }
    };

    number
        .checked_mul(in_secs)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("'{raw}': interval too large"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn successfully_parse_correct_args() {
        let cases = vec![
            ("23s", Duration::from_secs(23)),
            ("30m", Duration::from_mins(30)),
            ("24h", Duration::from_hours(24)),
            ("4d", Duration::from_hours(24 * 4)),
        ];

        for (given, expected) in cases {
            assert_eq!(parse_cleanup_interval(given).unwrap(), expected);
        }
    }
    #[test]
    fn trims_whitespaces() {
        assert_eq!(
            parse_cleanup_interval("  10s  ").unwrap(),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn rejects_zero() {
        let err = parse_cleanup_interval("0s").unwrap_err();
        assert!(err.contains("greater than zero"), "got: {err}");
    }

    #[test]
    fn rejects_units_larger_than_days() {
        assert!(parse_cleanup_interval("4w").is_err());
        assert!(parse_cleanup_interval("2y").is_err());
    }

    #[test]
    fn rejects_unknown_unit_with_helpful_message() {
        let err = parse_cleanup_interval("5x").unwrap_err();
        assert!(err.contains("unknown unit"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_inputs() {
        for bad in ["0s", "4w", "2y", "5", "s", "abc", "", "12.5h"] {
            assert!(
                parse_cleanup_interval(bad).is_err(),
                "should reject '{bad}'"
            );
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_cleanup_interval("abc").is_err());
        assert!(parse_cleanup_interval("").is_err());
        assert!(parse_cleanup_interval("12.5h").is_err()); // no decimals supported
    }

    #[test]
    fn rejects_overflow() {
        // value * secs_per overflows u64
        assert!(parse_cleanup_interval("99999999999999999999d").is_err());
    }
}
