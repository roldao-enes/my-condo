//! What each residency screen shows, as plain data. Pure functions of the directory, the search
//! text and "today": no Slint types here, so it is all unit-testable.

use condo_core::locale::{date, initials};
use condo_core::tags::normalize_name;
pub mod announcements;
pub mod finance;

use condo_db::residency::{Directory, Relationship, Unit};

pub fn residents_phrase(count: usize) -> String {
    match count {
        0 => "Sem residentes".to_owned(),
        1 => "1 residente".to_owned(),
        n => format!("{n} residentes"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitView {
    pub id: i64,
    pub label: String,
    pub floor: String,
    pub people: String,
    pub count: usize,
    pub summary: String,
    pub avatars: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationView {
    pub id: i64,
    pub name: String,
    pub initials: String,
    pub contact: String,
    pub unit_label: String,
    pub role: String,
    pub start: String,
    pub end: String,
    pub active: bool,
    pub email: String,
    pub phone: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatView {
    pub label: String,
    pub value: String,
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityView {
    pub name: String,
    pub text: String,
    pub date: String,
    pub departed: bool,
}

fn active_in(dir: &Directory, unit_id: i64) -> impl Iterator<Item = &Relationship> {
    dir.relationships
        .iter()
        .filter(move |r| r.unit_id == unit_id && r.end_date.is_none())
}

pub fn unit_view(dir: &Directory, unit: &Unit) -> UnitView {
    let people: Vec<&Relationship> = active_in(dir, unit.id).collect();
    UnitView {
        id: unit.id,
        label: unit.label.clone(),
        floor: unit.floor.clone(),
        people: people
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        count: people.len(),
        summary: residents_phrase(people.len()),
        avatars: people.iter().take(3).map(|p| initials(&p.name)).collect(),
    }
}

pub fn relation_view(r: &Relationship) -> RelationView {
    RelationView {
        id: r.id,
        name: r.name.clone(),
        initials: initials(&r.name),
        contact: r
            .email
            .clone()
            .or_else(|| r.phone.clone())
            .unwrap_or_else(|| "Sem contacto".to_owned()),
        unit_label: r.unit_label.clone(),
        role: r.role.label().to_owned(),
        start: date(r.start_date),
        end: r.end_date.map(date).unwrap_or_default(),
        active: r.end_date.is_none(),
        email: r.email.clone().unwrap_or_default(),
        phone: r.phone.clone().unwrap_or_default(),
    }
}

/// Accent- and case-insensitive "contains"; an empty query matches everything.
fn matches(query: &str, haystack: &str) -> bool {
    let needle = normalize_name(query);
    needle.is_empty() || normalize_name(haystack).contains(&needle)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overview {
    pub subtitle: String,
    pub stats: Vec<StatView>,
    pub units: Vec<UnitView>,
    pub activity: Vec<ActivityView>,
}

pub fn overview(dir: &Directory) -> Overview {
    let active: Vec<&Relationship> = dir
        .relationships
        .iter()
        .filter(|r| r.end_date.is_none())
        .collect();
    let occupied: std::collections::HashSet<i64> = active.iter().map(|r| r.unit_id).collect();
    let residents: std::collections::HashSet<i64> = active.iter().map(|r| r.resident_id).collect();
    let owners = active
        .iter()
        .filter(|r| r.role == condo_core::occupancy::ResidentRole::Owner)
        .count();
    let units = dir.units.len();

    Overview {
        subtitle: "O seu condomínio, num só lugar.".to_owned(),
        stats: vec![
            StatView {
                label: "Frações".into(),
                value: units.to_string(),
                caption: format!("{} sem residentes ativos", units - occupied.len()),
            },
            StatView {
                label: "Residentes ativos".into(),
                value: residents.len().to_string(),
                caption: format!(
                    "{owners} {} de propriedade",
                    if owners == 1 {
                        "relação"
                    } else {
                        "relações"
                    }
                ),
            },
            StatView {
                label: "Frações ocupadas".into(),
                value: format!("{}/{units}", occupied.len()),
                caption: "Com pelo menos uma relação ativa".into(),
            },
        ],
        units: dir
            .units
            .iter()
            .take(6)
            .map(|u| unit_view(dir, u))
            .collect(),
        activity: dir
            .relationships
            .iter()
            .take(5)
            .map(|r| ActivityView {
                name: r.name.clone(),
                text: format!(
                    "{} · {}",
                    if r.end_date.is_some() {
                        "Saída registada"
                    } else {
                        "Entrada registada"
                    },
                    r.unit_label
                ),
                date: date(r.end_date.unwrap_or(r.start_date)),
                departed: r.end_date.is_some(),
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitsPage {
    pub subtitle: String,
    pub rows: Vec<UnitView>,
    pub count_text: String,
    pub empty_title: String,
    pub empty_text: String,
}

pub fn units_page(dir: &Directory, query: &str) -> UnitsPage {
    let total = dir.units.len();
    let rows: Vec<UnitView> = dir
        .units
        .iter()
        .filter(|u| matches(query, &format!("{} {}", u.label, u.floor)))
        .map(|u| unit_view(dir, u))
        .collect();
    let searching = !normalize_name(query).is_empty();
    UnitsPage {
        subtitle: if total == 1 {
            "1 fração no condomínio".to_owned()
        } else {
            format!("{total} frações no condomínio")
        },
        count_text: if rows.len() == 1 {
            "1 resultado".to_owned()
        } else {
            format!("{} resultados", rows.len())
        },
        empty_title: if searching {
            "Nenhuma fração encontrada."
        } else {
            "Ainda não existem frações."
        }
        .to_owned(),
        empty_text: if searching {
            "Experimente outra pesquisa."
        } else {
            "Adicione a primeira fração do condomínio."
        }
        .to_owned(),
        rows,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentsPage {
    pub subtitle: String,
    pub rows: Vec<RelationView>,
    pub active_count: usize,
    pub history_count: usize,
    pub empty_title: String,
    pub empty_text: String,
}

pub fn residents_page(dir: &Directory, query: &str, history: bool) -> ResidentsPage {
    let active_count = dir
        .relationships
        .iter()
        .filter(|r| r.end_date.is_none())
        .count();
    let distinct: std::collections::HashSet<i64> = dir
        .relationships
        .iter()
        .filter(|r| r.end_date.is_none())
        .map(|r| r.resident_id)
        .collect();
    let rows: Vec<RelationView> = dir
        .relationships
        .iter()
        .filter(|r| r.end_date.is_some() == history)
        .filter(|r| {
            matches(
                query,
                &format!(
                    "{} {} {}",
                    r.name,
                    r.unit_label,
                    r.email.as_deref().unwrap_or("")
                ),
            )
        })
        .map(relation_view)
        .collect();
    let searching = !normalize_name(query).is_empty();
    let (empty_title, empty_text) = match (searching, history) {
        (true, _) => (
            "Nenhum residente encontrado.",
            "Experimente outra pesquisa.",
        ),
        (false, true) => (
            "Sem saídas registadas.",
            "As relações encerradas ficam guardadas aqui.",
        ),
        (false, false) => (
            "Ainda não existem residentes ativos.",
            "Associe um residente a uma fração.",
        ),
    };
    ResidentsPage {
        subtitle: format!(
            "{} {} com relação ativa",
            distinct.len(),
            if distinct.len() == 1 {
                "residente"
            } else {
                "residentes"
            }
        ),
        history_count: dir.relationships.len() - active_count,
        active_count,
        rows,
        empty_title: empty_title.to_owned(),
        empty_text: empty_text.to_owned(),
    }
}

/// Every relationship of one unit, active first-come order as stored (most recent first).
pub fn unit_detail(dir: &Directory, unit_id: i64) -> Option<(String, String, Vec<RelationView>)> {
    let unit = dir.units.iter().find(|u| u.id == unit_id)?;
    let rows = dir
        .relationships
        .iter()
        .filter(|r| r.unit_id == unit_id)
        .map(relation_view)
        .collect();
    Some((format!("Fração {}", unit.label), unit.floor.clone(), rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use condo_core::occupancy::ResidentRole;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn rel(
        id: i64,
        unit: (i64, &str),
        who: (i64, &str),
        role: ResidentRole,
        start: NaiveDate,
        end: Option<NaiveDate>,
    ) -> Relationship {
        Relationship {
            id,
            unit_id: unit.0,
            resident_id: who.0,
            unit_label: unit.1.into(),
            role,
            name: who.1.into(),
            email: None,
            phone: None,
            start_date: start,
            end_date: end,
        }
    }

    fn directory() -> Directory {
        let unit = |id, label: &str, floor: &str| Unit {
            id,
            label: label.into(),
            floor: floor.into(),
        };
        Directory {
            units: vec![
                unit(1, "1.o Esq.", "1"),
                unit(2, "R/C Dir.", "0"),
                unit(3, "2.o Dir.", "2"),
            ],
            residents: vec![],
            relationships: vec![
                rel(
                    4,
                    (1, "1.o Esq."),
                    (3, "Álvaro Maria"),
                    ResidentRole::Owner,
                    d(2025, 3, 1),
                    Some(d(2025, 5, 1)),
                ),
                rel(
                    3,
                    (2, "R/C Dir."),
                    (2, "Pedro Costa"),
                    ResidentRole::Tenant,
                    d(2025, 2, 1),
                    None,
                ),
                rel(
                    2,
                    (1, "1.o Esq."),
                    (1, "Maria Silva"),
                    ResidentRole::Owner,
                    d(2025, 1, 1),
                    None,
                ),
                rel(
                    1,
                    (1, "1.o Esq."),
                    (2, "Pedro Costa"),
                    ResidentRole::Occupant,
                    d(2025, 1, 1),
                    None,
                ),
            ],
        }
    }

    #[test]
    fn plural_forms() {
        assert_eq!(residents_phrase(0), "Sem residentes");
        assert_eq!(residents_phrase(1), "1 residente");
        assert_eq!(residents_phrase(3), "3 residentes");
    }

    #[test]
    fn overview_counts_people_and_units() {
        let view = overview(&directory());
        let stats: Vec<_> = view
            .stats
            .iter()
            .map(|s| (s.label.as_str(), s.value.as_str(), s.caption.as_str()))
            .collect();
        assert_eq!(
            stats,
            [
                ("Frações", "3", "1 sem residentes ativos"),
                // Pedro Costa lives in two units but counts once.
                ("Residentes ativos", "2", "1 relação de propriedade"),
                (
                    "Frações ocupadas",
                    "2/3",
                    "Com pelo menos uma relação ativa"
                ),
            ]
        );
        assert_eq!(view.units.len(), 3);
        assert_eq!(view.units[0].summary, "2 residentes");
        assert_eq!(view.units[0].people, "Maria Silva, Pedro Costa");
        assert_eq!(view.units[0].avatars, ["MS", "PC"]);
        assert_eq!(view.units[2].summary, "Sem residentes");
        assert_eq!(view.activity[0].text, "Saída registada · 1.o Esq.");
        assert!(view.activity[0].departed);
        assert_eq!(view.activity[0].date, "01/05/2025");
        assert_eq!(view.activity[1].text, "Entrada registada · R/C Dir.");
    }

    #[test]
    fn an_empty_condominium_has_zeroes_not_panics() {
        let view = overview(&Directory {
            units: vec![],
            residents: vec![],
            relationships: vec![],
        });
        assert_eq!(view.stats[2].value, "0/0");
        assert!(view.units.is_empty() && view.activity.is_empty());
    }

    #[test]
    fn overview_shows_at_most_six_units_and_five_movements() {
        let mut dir = directory();
        dir.units = (1..=9)
            .map(|id| Unit {
                id,
                label: format!("U{id}"),
                floor: "1".into(),
            })
            .collect();
        for n in 10..20 {
            dir.relationships.push(rel(
                n,
                (1, "U1"),
                (n, "Extra"),
                ResidentRole::Occupant,
                d(2025, 1, 1),
                None,
            ));
        }
        let view = overview(&dir);
        assert_eq!((view.units.len(), view.activity.len()), (6, 5));
    }

    #[test]
    fn unit_search_ignores_accents_case_and_punctuation() {
        let dir = directory();
        let labels = |q| {
            units_page(&dir, q)
                .rows
                .into_iter()
                .map(|u| u.label)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(""), ["1.o Esq.", "R/C Dir.", "2.o Dir."]);
        assert_eq!(labels("dir"), ["R/C Dir.", "2.o Dir."]);
        assert_eq!(labels("R/C"), ["R/C Dir."]);
        assert_eq!(labels("1.O esq"), ["1.o Esq."]);
        assert_eq!(labels("piso-2"), Vec::<String>::new()); // only label and floor text are searched
        assert_eq!(labels("2"), ["2.o Dir."]);
    }

    #[test]
    fn unit_page_texts() {
        let dir = directory();
        let page = units_page(&dir, "");
        assert_eq!(
            (page.subtitle.as_str(), page.count_text.as_str()),
            ("3 frações no condomínio", "3 resultados")
        );
        let none = units_page(&dir, "zzz");
        assert_eq!(
            (
                none.empty_title.as_str(),
                none.empty_text.as_str(),
                none.count_text.as_str()
            ),
            (
                "Nenhuma fração encontrada.",
                "Experimente outra pesquisa.",
                "0 resultados"
            )
        );
        let one = units_page(
            &Directory {
                units: vec![dir.units[0].clone()],
                ..dir.clone()
            },
            "",
        );
        assert_eq!(
            (one.subtitle.as_str(), one.count_text.as_str()),
            ("1 fração no condomínio", "1 resultado")
        );
        let empty = units_page(
            &Directory {
                units: vec![],
                residents: vec![],
                relationships: vec![],
            },
            "",
        );
        assert_eq!(empty.empty_title, "Ainda não existem frações.");
    }

    #[test]
    fn residents_split_into_active_and_history_with_counts() {
        let dir = directory();
        let active = residents_page(&dir, "", false);
        assert_eq!((active.active_count, active.history_count), (3, 1));
        assert_eq!(
            active
                .rows
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["Pedro Costa", "Maria Silva", "Pedro Costa"]
        );
        assert_eq!(active.subtitle, "2 residentes com relação ativa");
        let history = residents_page(&dir, "", true);
        assert_eq!(history.rows.len(), 1);
        assert_eq!(
            (
                history.rows[0].start.as_str(),
                history.rows[0].end.as_str(),
                history.rows[0].active
            ),
            ("01/03/2025", "01/05/2025", false)
        );
        assert_eq!(history.rows[0].role, "Proprietário");
        assert_eq!(history.rows[0].contact, "Sem contacto");
    }

    #[test]
    fn resident_search_matches_name_unit_and_email_without_accents() {
        let mut dir = directory();
        dir.relationships[2].email = Some("maria@example.test".into());
        let names = |q, h| {
            residents_page(&dir, q, h)
                .rows
                .into_iter()
                .map(|r| r.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("MARIA", false), ["Maria Silva"]);
        assert_eq!(names("example", false), ["Maria Silva"]);
        assert_eq!(names("r/c", false), ["Pedro Costa"]);
        assert_eq!(names("alvaro", true), ["Álvaro Maria"]);
        assert_eq!(names("alvaro", false), Vec::<String>::new()); // history only shows in history
        let page = residents_page(&dir, "zzz", true);
        assert_eq!(page.empty_title, "Nenhum residente encontrado.");
        assert_eq!(
            residents_page(&dir, "", true).rows[0].contact,
            "Sem contacto"
        );
        assert_eq!(
            residents_page(
                &Directory {
                    relationships: vec![],
                    ..dir.clone()
                },
                "",
                true
            )
            .empty_title,
            "Sem saídas registadas."
        );
        assert_eq!(
            residents_page(
                &Directory {
                    relationships: vec![],
                    ..dir.clone()
                },
                "",
                false
            )
            .empty_title,
            "Ainda não existem residentes ativos."
        );
    }

    #[test]
    fn contact_prefers_email_then_phone() {
        let mut r = rel(
            1,
            (1, "A"),
            (1, "Ana"),
            ResidentRole::Owner,
            d(2025, 1, 1),
            None,
        );
        assert_eq!(relation_view(&r).contact, "Sem contacto");
        r.phone = Some("912345678".into());
        assert_eq!(relation_view(&r).contact, "912345678");
        r.email = Some("ana@example.test".into());
        assert_eq!(relation_view(&r).contact, "ana@example.test");
    }

    #[test]
    fn unit_detail_lists_active_and_past_relationships() {
        let dir = directory();
        let (title, floor, rows) = unit_detail(&dir, 1).unwrap();
        assert_eq!(
            (title.as_str(), floor.as_str(), rows.len()),
            ("Fração 1.o Esq.", "1", 3)
        );
        assert!(unit_detail(&dir, 99).is_none());
    }
}
