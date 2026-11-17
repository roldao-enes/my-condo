//! Occupancy date rules. Ported from `app/residency/domain/occupancy.ts`.

use chrono::NaiveDate;

use crate::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentRole {
    Owner,
    Tenant,
    Occupant,
}

impl ResidentRole {
    /// The value stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Tenant => "tenant",
            Self::Occupant => "occupant",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "tenant" => Some(Self::Tenant),
            "occupant" => Some(Self::Occupant),
            _ => None,
        }
    }

    /// Portuguese display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Owner => "Proprietário",
            Self::Tenant => "Arrendatário",
            Self::Occupant => "Ocupante",
        }
    }
}

/// Strict `YYYY-MM-DD`: exactly four-two-two digits and a real calendar day.
pub fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    let b = value.as_bytes();
    let shaped = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shaped {
        return None;
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

/// `end_date` of `None` or an empty string means the relationship is ongoing.
pub fn validate_occupancy_dates(
    start_date: &str,
    end_date: Option<&str>,
) -> Result<(), DomainError> {
    let invalid = || DomainError::new("Introduza uma data valida.");
    let start = parse_iso_date(start_date).ok_or_else(invalid)?;
    let end = match end_date.filter(|value| !value.is_empty()) {
        Some(value) => Some(parse_iso_date(value).ok_or_else(invalid)?),
        None => None,
    };
    if end.is_some_and(|end| end < start) {
        return Err(DomainError::new(
            "A data de saida nao pode ser anterior a data de entrada.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_ongoing_relationship_and_a_same_day_departure() {
        validate_occupancy_dates("2026-10-27", None).unwrap();
        validate_occupancy_dates("2026-10-27", Some("")).unwrap();
        validate_occupancy_dates("2026-10-27", Some("2026-10-27")).unwrap();
    }

    #[test]
    fn rejects_departure_before_arrival() {
        assert!(validate_occupancy_dates("2026-10-27", Some("2026-10-26")).is_err());
    }

    #[test]
    fn rejects_malformed_and_impossible_calendar_dates() {
        for date in [
            "2026-02-30",
            "2025-02-29",
            "27/10/2026",
            "invalid",
            "2026-1-05",
        ] {
            assert!(validate_occupancy_dates(date, None).is_err(), "{date}");
        }
        assert!(validate_occupancy_dates("2026-01-01", Some("2026-02-30")).is_err());
        validate_occupancy_dates("2024-02-29", None).unwrap();
    }
}
