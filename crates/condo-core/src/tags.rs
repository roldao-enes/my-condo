//! Name registry: normalization and tag matching. Ported from `app/finances/domain/tags.ts`.

use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    Exact,
    Contains,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagCategory {
    Recurring,
    Extraordinary,
    ResidentPayment,
}

impl MatchType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Contains => "contains",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "exact" => Some(Self::Exact),
            "contains" => Some(Self::Contains),
            _ => None,
        }
    }
}

impl TagCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recurring => "recurring",
            Self::Extraordinary => "extraordinary",
            Self::ResidentPayment => "resident_payment",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "recurring" => Some(Self::Recurring),
            "extraordinary" => Some(Self::Extraordinary),
            "resident_payment" => Some(Self::ResidentPayment),
            _ => None,
        }
    }

    /// Resident payments are credits; expense categories are debits.
    pub fn direction(self) -> &'static str {
        match self {
            Self::ResidentPayment => "credit",
            _ => "debit",
        }
    }

    /// Portuguese display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Recurring => "Recorrente",
            Self::Extraordinary => "Extraordinária",
            Self::ResidentPayment => "Pagamento de residente",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinanceTag {
    pub id: i64,
    pub name: String,
    /// Already normalized (see [`normalize_name`]).
    pub pattern: String,
    pub match_type: MatchType,
    pub category: TagCategory,
    pub resident_id: Option<i64>,
    pub unit_id: Option<i64>,
}

/// Uppercases, strips diacritics and collapses every non-alphanumeric run into one space.
pub fn normalize_name(value: &str) -> String {
    let upper: String = value
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_uppercase)
        .collect();
    upper
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Picks the single most specific tag for a bank movement, or `None` when there is no
/// match or the match is ambiguous.
pub fn match_tag<'a>(
    description: &str,
    amount: i64,
    tags: &'a [FinanceTag],
) -> Option<&'a FinanceTag> {
    let name = normalize_name(description);
    let padded = format!(" {name} ");
    let mut matches: Vec<&FinanceTag> = tags
        .iter()
        .filter(|tag| {
            let direction_ok = if tag.category == TagCategory::ResidentPayment {
                amount > 0
            } else {
                amount < 0
            };
            direction_ok
                && match tag.match_type {
                    MatchType::Exact => name == tag.pattern,
                    MatchType::Contains => padded.contains(&format!(" {} ", tag.pattern)),
                }
        })
        .collect();
    // Exact before contains, then longer pattern first. Stable, like the JS sort.
    matches.sort_by(|a, b| {
        let exact = |t: &FinanceTag| t.match_type == MatchType::Exact;
        exact(b)
            .cmp(&exact(a))
            .then(b.pattern.chars().count().cmp(&a.pattern.chars().count()))
    });

    let first = *matches.first()?;
    if let Some(second) = matches.get(1) {
        if first.match_type == second.match_type
            && first.pattern.chars().count() == second.pattern.chars().count()
        {
            return None;
        }
    }
    if first.match_type == MatchType::Contains {
        let first_padded = format!(" {} ", first.pattern);
        if matches
            .iter()
            .any(|tag| !first_padded.contains(&format!(" {} ", tag.pattern)))
        {
            return None;
        }
    }
    Some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag() -> FinanceTag {
        FinanceTag {
            id: 1,
            name: "Maintenance".into(),
            pattern: "MARINA SEQUEIRA".into(),
            match_type: MatchType::Contains,
            category: TagCategory::Recurring,
            resident_id: None,
            unit_id: None,
        }
    }

    #[test]
    fn matches_normalized_names_with_accents_punctuation_and_changing_references() {
        assert_eq!(
            normalize_name("  TRF.P/ Mária   Silva "),
            "TRF P MARIA SILVA"
        );
        let tags = [tag()];
        assert_eq!(
            match_tag("TRF.P/ Marina Sequeira REF 123", -7500, &tags).map(|t| t.id),
            Some(1)
        );
        assert!(match_tag("TRF.P/ Marina Sequeiraa", -7500, &tags).is_none());
        assert!(match_tag("TRF.P/ Marina Sequeira", 7500, &tags).is_none());
    }

    #[test]
    fn prefers_exact_then_specific_names_and_leaves_ambiguous_names_unclassified() {
        let exact = FinanceTag {
            id: 2,
            pattern: "TRF P MARINA SEQUEIRA".into(),
            match_type: MatchType::Exact,
            ..tag()
        };
        assert_eq!(
            match_tag("TRF.P/ Marina Sequeira", -7500, &[tag(), exact]).map(|t| t.id),
            Some(2)
        );

        let shorter = FinanceTag {
            id: 3,
            pattern: "MARINA".into(),
            ..tag()
        };
        assert_eq!(
            match_tag("Marina Sequeira", -7500, &[shorter, tag()]).map(|t| t.id),
            Some(1)
        );

        let other = FinanceTag {
            id: 4,
            pattern: "MARIA OLIVEIRA".into(),
            ..tag()
        };
        assert!(match_tag("Marina Sequeira Maria Oliveira", -7500, &[tag(), other]).is_none());

        let payer = FinanceTag {
            category: TagCategory::ResidentPayment,
            resident_id: Some(1),
            unit_id: Some(1),
            ..tag()
        };
        assert_eq!(
            match_tag("Marina Sequeira", 10_000, &[payer]).and_then(|t| t.resident_id),
            Some(1)
        );
    }

    #[test]
    fn normalizes_unicode_edge_cases() {
        assert_eq!(normalize_name("ção / ÀÉÎ"), "CAO AEI");
        assert_eq!(normalize_name("---"), "");
        assert_eq!(normalize_name("a1  b2"), "A1 B2");
    }
}
