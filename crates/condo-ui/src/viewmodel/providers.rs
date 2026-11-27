//! What the Fornecedores screen shows: each provider with its details and the names it appears
//! under in the bank statement, as plain data.

use condo_core::tags::MatchType;
use condo_db::providers::Provider;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRow {
    pub id: i64,
    pub pattern: String,
    pub matching: &'static str,
    pub category: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    pub id: i64,
    pub name: String,
    pub kind: &'static str,
    pub contact: String,
    pub email: String,
    pub phone: String,
    /// In groups of four, or empty.
    pub iban: String,
    pub note: String,
    pub names: Vec<NameRow>,
}

pub fn subtitle(count: usize) -> String {
    match count {
        0 => "Nenhum fornecedor registado".to_owned(),
        1 => "1 fornecedor".to_owned(),
        n => format!("{n} fornecedores"),
    }
}

pub fn matching_label(match_type: MatchType) -> &'static str {
    match match_type {
        MatchType::Exact => "Descrição completa",
        MatchType::Contains => "Contém nome",
    }
}

pub fn rows(providers: &[Provider]) -> Vec<ProviderRow> {
    providers
        .iter()
        .map(|p| ProviderRow {
            id: p.id,
            name: p.name.clone(),
            kind: p.kind.label(),
            contact: p.contact_name.clone().unwrap_or_default(),
            email: p.email.clone().unwrap_or_default(),
            phone: p.phone.clone().unwrap_or_default(),
            iban: p.iban_display().unwrap_or_default(),
            note: p.note.clone().unwrap_or_default(),
            names: p
                .tags
                .iter()
                .map(|t| NameRow {
                    id: t.id,
                    pattern: t.pattern.clone(),
                    matching: matching_label(t.match_type),
                    category: t.category.label(),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use condo_core::providers::ProviderKind;
    use condo_core::tags::TagCategory;
    use condo_db::providers::ProviderTag;

    #[test]
    fn shows_details_with_the_iban_in_groups_and_blanks_for_what_is_missing() {
        let provider = Provider {
            id: 1,
            kind: ProviderKind::Electricity,
            name: "Ária".into(),
            contact_name: None,
            email: Some("geral@aria.example.test".into()),
            phone: None,
            iban: Some("PT50000201231234567890154".into()),
            note: None,
            tags: vec![ProviderTag {
                id: 7,
                pattern: "ARIA ENERGIA".into(),
                match_type: MatchType::Contains,
                category: TagCategory::Recurring,
            }],
        };
        let rows = rows(&[provider]);
        assert_eq!(rows[0].kind, "Eletricidade");
        assert_eq!(rows[0].iban, "PT50 0002 0123 1234 5678 9015 4");
        assert_eq!((rows[0].contact.as_str(), rows[0].phone.as_str()), ("", ""));
        assert_eq!(rows[0].names[0].matching, "Contém nome");
        assert_eq!(rows[0].names[0].category, "Recorrente");
        assert_eq!(subtitle(0), "Nenhum fornecedor registado");
        assert_eq!(subtitle(1), "1 fornecedor");
        assert_eq!(subtitle(3), "3 fornecedores");
    }
}
