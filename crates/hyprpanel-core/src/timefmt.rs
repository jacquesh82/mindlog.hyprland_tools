//! Horodatage ISO-8601 UTC sans dépendance externe.

use std::time::{SystemTime, UNIX_EPOCH};

/// Convertit un instant en chaîne `YYYYmmdd-HHMMSS` (noms de backups).
pub fn compact(ts: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = parts(ts);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// Convertit un instant en chaîne `YYYY-mm-ddTHH:MM:SSZ` (journaux, événements).
pub fn iso(ts: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = parts(ts);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn parts(ts: SystemTime) -> (i64, u32, u32, u32, u32, u32) {
    let secs = ts
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Algorithme « civil_from_days » de Howard Hinnant (domaine public).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn epoch_et_dates_connues() {
        assert_eq!(iso(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        // 2026-11-07 23:00:00 UTC (vérifié avec `date -u -d`)
        let t = UNIX_EPOCH + Duration::from_secs(1_794_092_400);
        assert_eq!(iso(t), "2026-11-07T23:00:00Z");
        assert_eq!(compact(t), "20261107-230000");
        // 2024-02-29 (bissextile)
        let b = UNIX_EPOCH + Duration::from_secs(1_709_164_800);
        assert_eq!(iso(b), "2024-02-29T00:00:00Z");
    }
}
