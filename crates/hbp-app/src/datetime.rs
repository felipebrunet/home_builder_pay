//! Local calendar fields for product deadlines. Unix stays internal.

use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike};

pub const MONTHS_ES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

const WEEKDAYS_ES: [&str; 7] = [
    "lunes",
    "martes",
    "miércoles",
    "jueves",
    "viernes",
    "sábado",
    "domingo",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadlineFields {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl DeadlineFields {
    pub fn days_from_now(days: i64) -> Self {
        let dt = Local::now() + chrono::Duration::days(days);
        Self {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: 0,
        }
    }

    pub fn from_unix(unix: u32) -> Self {
        match Local.timestamp_opt(unix as i64, 0).single() {
            Some(dt) => Self {
                year: dt.year(),
                month: dt.month(),
                day: dt.day(),
                hour: dt.hour(),
                minute: dt.minute(),
            },
            None => Self::days_from_now(7),
        }
    }

    pub fn to_unix(&self) -> Result<u32, String> {
        let date = NaiveDate::from_ymd_opt(self.year, self.month, self.day).ok_or_else(|| {
            format!(
                "fecha inválida {}-{:02}-{:02}",
                self.year, self.month, self.day
            )
        })?;
        let naive = date
            .and_hms_opt(self.hour, self.minute, 0)
            .ok_or_else(|| format!("hora inválida {:02}:{:02}", self.hour, self.minute))?;
        let dt = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| "esa fecha y hora no existen en tu zona horaria".to_string())?;
        let ts = dt.timestamp();
        if ts < 500_000_000 || ts > i64::from(u32::MAX) {
            return Err("la fecha queda fuera del rango usable".into());
        }
        Ok(ts as u32)
    }

    pub fn preview_es(&self) -> String {
        match self.to_unix() {
            Ok(u) => format_unix_local_es(u),
            Err(e) => format!("revisa la fecha: {e}"),
        }
    }
}

pub fn format_unix_local_es(unix: u32) -> String {
    let Some(dt) = Local.timestamp_opt(unix as i64, 0).single() else {
        return "fecha no válida".into();
    };
    let wd = WEEKDAYS_ES[dt.weekday().num_days_from_monday() as usize];
    format!(
        "{wd} {} de {} de {}, {:02}:{:02} (hora local)",
        dt.day(),
        MONTHS_ES[dt.month0() as usize],
        dt.year(),
        dt.hour(),
        dt.minute()
    )
}

pub fn validate_deadline_order(t1: u32, t2: u32) -> Result<(), String> {
    if t2 <= t1 {
        return Err("el segundo plazo tiene que ser después del primero".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_local_date_converts_and_previews() {
        let f = DeadlineFields {
            year: 2030,
            month: 6,
            day: 15,
            hour: 18,
            minute: 30,
        };
        let u = f.to_unix().expect("valid");
        assert!(u >= 500_000_000);
        let preview = f.preview_es();
        assert!(preview.contains("hora local"));
        assert!(preview.contains("junio"));
        assert!(!preview.to_ascii_lowercase().contains("unix"));
        let back = DeadlineFields::from_unix(u);
        assert_eq!(back.year, 2030);
        assert_eq!(back.month, 6);
        assert_eq!(back.day, 15);
        assert_eq!(back.hour, 18);
        assert_eq!(back.minute, 30);
    }

    #[test]
    fn invalid_date_is_rejected() {
        let f = DeadlineFields {
            year: 2026,
            month: 2,
            day: 30,
            hour: 12,
            minute: 0,
        };
        let err = f.to_unix().unwrap_err();
        assert!(err.contains("inválida"));
        assert!(!err.contains("unix"));
    }

    #[test]
    fn second_deadline_must_be_later() {
        let err = validate_deadline_order(1_800_000_000, 1_700_000_000).unwrap_err();
        assert!(err.contains("después"));
        assert!(validate_deadline_order(1_700_000_000, 1_800_000_000).is_ok());
    }

    #[test]
    fn format_never_shows_raw_unix() {
        let s = format_unix_local_es(1_800_000_000);
        assert!(s.contains("hora local") || s == "fecha no válida");
        assert!(!s.chars().all(|c| c.is_ascii_digit()));
        assert!(!s.contains("1_800_000_000"));
        assert!(!s.contains("1800000000"));
    }
}
