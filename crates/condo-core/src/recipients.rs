//! Announcement recipients. Ported from `app/mail/domain/recipients.ts`.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientRow {
    pub person_id: i64,
    pub name: String,
    pub email: Option<String>,
    pub unit_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub person_id: i64,
    pub name: String,
    pub email: String,
    pub unit_labels: Vec<String>,
}

/// Equivalent of `/^[^\s@]+@[^\s@]+\.[^\s@]+$/`.
pub fn is_valid_email(value: &str) -> bool {
    let plain = |s: &str| !s.is_empty() && !s.contains(|c: char| c.is_whitespace() || c == '@');
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    // A dot with at least one character on both sides, inside the domain.
    plain(local)
        && plain(domain)
        && domain
            .char_indices()
            .any(|(i, c)| c == '.' && i > 0 && i + 1 < domain.len())
}

/// Collapses active occupancy rows into one recipient per distinct email address.
/// People without a usable email are skipped; units are merged per recipient.
pub fn build_recipients(rows: &[RecipientRow]) -> Vec<Recipient> {
    let mut recipients: Vec<Recipient> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let Some(email) = row.email.as_deref().map(|e| e.trim().to_lowercase()) else {
            continue;
        };
        if email.is_empty() || email.chars().count() > 254 || !is_valid_email(&email) {
            continue;
        }
        match index.get(&email) {
            None => {
                index.insert(email.clone(), recipients.len());
                recipients.push(Recipient {
                    person_id: row.person_id,
                    name: row.name.clone(),
                    email,
                    unit_labels: vec![row.unit_label.clone()],
                });
            }
            Some(&i) => {
                if !recipients[i].unit_labels.contains(&row.unit_label) {
                    recipients[i].unit_labels.push(row.unit_label.clone());
                }
            }
        }
    }
    recipients
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, name: &str, email: Option<&str>, unit: &str) -> RecipientRow {
        RecipientRow {
            person_id: id,
            name: name.into(),
            email: email.map(Into::into),
            unit_label: unit.into(),
        }
    }

    #[test]
    fn skips_people_without_a_valid_email() {
        let recipients = build_recipients(&[
            row(1, "Ana", None, "1A"),
            row(2, "Rui", Some("not-an-email"), "1B"),
            row(3, "Eva", Some("eva@example.test"), "2A"),
        ]);
        let emails: Vec<_> = recipients.iter().map(|r| r.email.as_str()).collect();
        assert_eq!(emails, ["eva@example.test"]);
    }

    #[test]
    fn merges_units_and_duplicate_addresses_case_insensitively() {
        let recipients = build_recipients(&[
            row(1, "Ana", Some("Ana@Example.test"), "1A"),
            row(1, "Ana", Some("ana@example.test"), "2B"),
            row(1, "Ana", Some("ana@example.test"), "1A"),
            row(4, "Ana (spouse)", Some(" ANA@example.test "), "1A"),
        ]);
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0].email, "ana@example.test");
        assert_eq!(recipients[0].person_id, 1);
        assert_eq!(recipients[0].unit_labels, ["1A", "2B"]);
    }

    #[test]
    fn email_shape_matches_the_original_regex() {
        for ok in ["a@b.c", "a.b@c.d.e", "a@b.c.d"] {
            assert!(is_valid_email(ok), "{ok}");
        }
        for bad in [
            "a@b", "a@.c", "a@b.", "@b.c", "a@@b.c", "a b@c.d", "a@b .c", "",
        ] {
            assert!(!is_valid_email(bad), "{bad}");
        }
        let too_long = format!("{}@example.test", "a".repeat(250));
        assert!(build_recipients(&[row(1, "X", Some(&too_long), "1A")]).is_empty());
    }
}
