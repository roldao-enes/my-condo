//! Providers of services to the condominium (electricity, water, cleaning, bank, insurance):
//! their kinds, and the checks on the contact details a provider holds.

use crate::DomainError;

/// What a provider supplies. There is one provider of each kind, except [`ProviderKind::Other`],
/// which stands for everything the list does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Electricity,
    Water,
    Cleaning,
    Bank,
    Insurance,
    Other,
}

impl ProviderKind {
    pub const ALL: [Self; 6] = [
        Self::Electricity,
        Self::Water,
        Self::Cleaning,
        Self::Bank,
        Self::Insurance,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Electricity => "electricity",
            Self::Water => "water",
            Self::Cleaning => "cleaning",
            Self::Bank => "bank",
            Self::Insurance => "insurance",
            Self::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Portuguese display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Electricity => "Eletricidade",
            Self::Water => "Água",
            Self::Cleaning => "Limpeza",
            Self::Bank => "Banco",
            Self::Insurance => "Seguro",
            Self::Other => "Outro",
        }
    }

    /// Whether only one provider of this kind may exist.
    pub fn is_unique(self) -> bool {
        self != Self::Other
    }
}

/// Length of an IBAN by country, for the countries a condominium is likely to deal with. Other
/// countries are only held to the general 15 to 34 characters and the checksum.
fn expected_length(country: &str) -> Option<usize> {
    match country {
        "PT" => Some(25),
        "ES" => Some(24),
        "FR" => Some(27),
        "DE" => Some(22),
        "GB" => Some(22),
        "IT" => Some(27),
        "NL" => Some(18),
        "BE" => Some(16),
        "LU" => Some(20),
        "IE" => Some(22),
        "CH" => Some(21),
        _ => None,
    }
}

fn invalid_iban() -> DomainError {
    DomainError::new("O IBAN indicado não é válido.")
}

/// The IBAN without spaces and in capitals, once it passes the length and checksum (ISO 7064
/// mod 97-10) checks. Blank input is not an IBAN: callers decide whether it is optional.
pub fn normalize_iban(value: &str) -> Result<String, DomainError> {
    let iban: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_uppercase)
        .collect();
    if !(15..=34).contains(&iban.len()) || !iban.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(invalid_iban());
    }
    let (country, rest) = iban.split_at(2);
    let (check, _) = rest.split_at(2);
    if !country.chars().all(|c| c.is_ascii_uppercase())
        || !check.chars().all(|c| c.is_ascii_digit())
    {
        return Err(invalid_iban());
    }
    if expected_length(country).is_some_and(|length| length != iban.len()) {
        return Err(invalid_iban());
    }
    // Move the first four characters to the end, read letters as 10..35, and the number must
    // leave a remainder of 1 when divided by 97. Done digit by digit, so it never overflows.
    let remainder = iban[4..]
        .chars()
        .chain(iban[..4].chars())
        .try_fold(0u32, |remainder, c| {
            let digit = c.to_digit(36)?;
            Some(if digit >= 10 {
                (remainder * 100 + digit) % 97
            } else {
                (remainder * 10 + digit) % 97
            })
        });
    if remainder == Some(1) {
        Ok(iban)
    } else {
        Err(invalid_iban())
    }
}

/// An IBAN in groups of four, for reading: `PT50 0002 0123 1234 5678 9015 4`.
pub fn format_iban(iban: &str) -> String {
    iban.chars()
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|group| group.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A plausibility check for an email address, not a deliverability one: something before and
/// after a single `@`, a dot in the domain, and no spaces.
pub fn check_email(value: &str) -> Result<(), DomainError> {
    let invalid = || DomainError::new("O email indicado não é válido.");
    let (local, domain) = value.split_once('@').ok_or_else(invalid)?;
    let well_formed = !local.is_empty()
        && !domain.contains('@')
        && !value.chars().any(char::is_whitespace)
        && domain.split('.').all(|part| !part.is_empty())
        && domain.contains('.');
    if well_formed {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// A telephone number holds digits, with an optional leading `+` and the usual separators, and
/// between 6 and 15 digits.
pub fn check_phone(value: &str) -> Result<(), DomainError> {
    let invalid = || DomainError::new("O telefone indicado não é válido.");
    let digits = value.chars().filter(char::is_ascii_digit).count();
    let allowed = value
        .chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_digit() || " .-()".contains(c) || (i == 0 && c == '+'));
    if allowed && (6..=15).contains(&digits) {
        Ok(())
    } else {
        Err(invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_ibans_whatever_their_spacing_or_case() {
        for iban in [
            "PT50 0002 0123 1234 5678 9015 4",
            "pt50000201231234567890154",
            "ES91 2100 0418 4502 0005 1332",
            "DE89 3704 0044 0532 0130 00",
            "GB82 WEST 1234 5698 7654 32",
        ] {
            assert!(normalize_iban(iban).is_ok(), "{iban}");
        }
        assert_eq!(
            normalize_iban("pt50 0002 0123 1234 5678 9015 4").unwrap(),
            "PT50000201231234567890154"
        );
    }

    #[test]
    fn refuses_a_wrong_checksum_length_or_character() {
        for iban in [
            "",
            "PT50 0002 0123 1234 5678 9015 5",
            "PT50 0002 0123 1234 5678 9015",
            "PT50 0002 0123 1234 5678 9015 44",
            "XX00",
            "PT50 0002 0123 1234 5678 901É 4",
            "5050 0002 0123 1234 5678 9015 4",
        ] {
            assert!(normalize_iban(iban).is_err(), "{iban}");
        }
    }

    #[test]
    fn formats_in_groups_of_four() {
        assert_eq!(
            format_iban("PT50000201231234567890154"),
            "PT50 0002 0123 1234 5678 9015 4"
        );
    }

    #[test]
    fn checks_email_and_phone_plausibility() {
        assert!(check_email("geral@edp.pt").is_ok());
        for bad in [
            "",
            "geral",
            "geral@",
            "@edp.pt",
            "a b@edp.pt",
            "a@edp",
            "a@@edp.pt",
        ] {
            assert!(check_email(bad).is_err(), "{bad}");
        }
        assert!(check_phone("+351 210 000 000").is_ok());
        assert!(check_phone("210-000-000").is_ok());
        for bad in ["", "12", "telefone", "21 000 00x 00", "1234567890123456"] {
            assert!(check_phone(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn kinds_round_trip_and_only_other_repeats() {
        for kind in ProviderKind::ALL {
            assert_eq!(ProviderKind::parse(kind.as_str()), Some(kind));
        }
        assert!(ProviderKind::Water.is_unique());
        assert!(!ProviderKind::Other.is_unique());
    }
}
