//! What each residency screen shows, as plain data. Pure functions of the directory, the search
//! text and "today": no Slint types here, so it is all unit-testable.

use condo_core::locale::{date, initials};
use condo_core::occupancy::UnitRole;
use condo_core::tags::normalize_name;
pub mod announcements;
pub mod finance;

use std::collections::HashSet;

use condo_db::residency::{Directory, Relationship, Unit};

/// A unit with who owns it and, for information only, who lives in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitView {
    pub id: i64,
    pub label: String,
    pub floor: String,
    /// The owner's name ("" when none is recorded).
    pub owner: String,
    pub owner_initials: String,
    /// Who lives there ("" when nobody is recorded); not necessarily the owner.
    pub resident: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationView {
    pub id: i64,
    pub person_id: i64,
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

fn active_in(dir: &Directory, unit_id: i64, role: UnitRole) -> impl Iterator<Item = &Relationship> {
    dir.relationships
        .iter()
        .filter(move |r| r.unit_id == unit_id && r.role == role && r.end_date.is_none())
}

fn names(people: &[&Relationship]) -> String {
    people
        .iter()
        .map(|p| p.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn unit_view(dir: &Directory, unit: &Unit) -> UnitView {
    // A database upgraded from before this rule can hold several; they are all shown.
    let owners: Vec<&Relationship> = active_in(dir, unit.id, UnitRole::Owner).collect();
    let residents: Vec<&Relationship> = active_in(dir, unit.id, UnitRole::Resident).collect();
    UnitView {
        id: unit.id,
        label: unit.label.clone(),
        floor: unit.floor.clone(),
        owner: names(&owners),
        owner_initials: owners
            .first()
            .map(|o| initials(&o.name))
            .unwrap_or_default(),
        resident: names(&residents),
    }
}

pub fn relation_view(r: &Relationship) -> RelationView {
    RelationView {
        id: r.id,
        person_id: r.person_id,
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
    let active = |role: UnitRole| {
        dir.relationships
            .iter()
            .filter(move |r| r.role == role && r.end_date.is_none())
    };
    let owned: HashSet<i64> = active(UnitRole::Owner).map(|r| r.unit_id).collect();
    let owners: HashSet<i64> = active(UnitRole::Owner).map(|r| r.person_id).collect();
    let lived_in: HashSet<i64> = active(UnitRole::Resident).map(|r| r.unit_id).collect();
    let units = dir.units.len();

    Overview {
        subtitle: "O seu condomínio, num só lugar.".to_owned(),
        stats: vec![
            StatView {
                label: "Frações".into(),
                value: units.to_string(),
                caption: format!("{} sem proprietário", units - owned.len()),
            },
            StatView {
                label: "Proprietários".into(),
                value: owners.len().to_string(),
                caption: "Membros do condomínio".into(),
            },
            StatView {
                label: "Frações com residente".into(),
                value: format!("{}/{units}", lived_in.len()),
                caption: "Residente indicado, só a título informativo".into(),
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
                    match (r.role, r.end_date.is_some()) {
                        (UnitRole::Owner, false) => "Novo proprietário",
                        (UnitRole::Owner, true) => "Saída de proprietário",
                        (UnitRole::Resident, false) => "Novo residente",
                        (UnitRole::Resident, true) => "Saída de residente",
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
pub struct PeoplePage {
    pub subtitle: String,
    pub rows: Vec<RelationView>,
    /// Active and past relationships of the shown role.
    pub active_count: usize,
    pub history_count: usize,
    /// Active owners and active residents, for the tab labels.
    pub owners_count: usize,
    pub residents_count: usize,
    pub empty_title: String,
    pub empty_text: String,
}

/// One tab of the people page: the owners, or the residents, active or past.
pub fn people_page(dir: &Directory, query: &str, history: bool, role: UnitRole) -> PeoplePage {
    let active_of = |role: UnitRole| {
        dir.relationships
            .iter()
            .filter(move |r| r.role == role && r.end_date.is_none())
            .count()
    };
    let in_tab = || dir.relationships.iter().filter(|r| r.role == role);
    let active_count = in_tab().filter(|r| r.end_date.is_none()).count();
    let rows: Vec<RelationView> = in_tab()
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
    let (noun, none_yet) = match role {
        UnitRole::Owner => ("proprietário", "Indique o proprietário de uma fração."),
        UnitRole::Resident => ("residente", "Indique quem vive numa fração."),
    };
    let (empty_title, empty_text) = match (searching, history) {
        (true, _) => (
            "Nenhuma pessoa encontrada.".to_owned(),
            "Experimente outra pesquisa.".to_owned(),
        ),
        (false, true) => (
            "Sem saídas registadas.".to_owned(),
            "As relações encerradas ficam guardadas aqui.".to_owned(),
        ),
        (false, false) => (
            format!("Ainda não existem {noun}s registados."),
            none_yet.to_owned(),
        ),
    };
    PeoplePage {
        subtitle: if active_count == 1 {
            format!("1 {noun} ativo")
        } else {
            format!("{active_count} {noun}s ativos")
        },
        history_count: in_tab().count() - active_count,
        active_count,
        owners_count: active_of(UnitRole::Owner),
        residents_count: active_of(UnitRole::Resident),
        rows,
        empty_title,
        empty_text,
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

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn rel(
        id: i64,
        unit: (i64, &str),
        who: (i64, &str),
        role: UnitRole,
        start: NaiveDate,
        end: Option<NaiveDate>,
    ) -> Relationship {
        Relationship {
            id,
            unit_id: unit.0,
            person_id: who.0,
            unit_label: unit.1.into(),
            role,
            name: who.1.into(),
            email: None,
            phone: None,
            start_date: start,
            end_date: end,
        }
    }

    /// Unit 1: owned by Maria (Álvaro sold it), Pedro lives there. Unit 2: owned by Rui and
    /// rented to Pedro too. Unit 3: nobody recorded.
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
            people: vec![],
            relationships: vec![
                rel(
                    4,
                    (1, "1.o Esq."),
                    (3, "Álvaro Maria"),
                    UnitRole::Owner,
                    d(2025, 3, 1),
                    Some(d(2025, 5, 1)),
                ),
                rel(
                    3,
                    (2, "R/C Dir."),
                    (2, "Pedro Costa"),
                    UnitRole::Resident,
                    d(2025, 2, 1),
                    None,
                ),
                rel(
                    2,
                    (1, "1.o Esq."),
                    (1, "Maria Silva"),
                    UnitRole::Owner,
                    d(2025, 1, 1),
                    None,
                ),
                rel(
                    1,
                    (1, "1.o Esq."),
                    (2, "Pedro Costa"),
                    UnitRole::Resident,
                    d(2025, 1, 1),
                    None,
                ),
                rel(
                    5,
                    (2, "R/C Dir."),
                    (4, "Rui Carvalho"),
                    UnitRole::Owner,
                    d(2024, 6, 1),
                    None,
                ),
            ],
        }
    }

    #[test]
    fn overview_counts_owners_and_units_and_keeps_residents_informative() {
        let view = overview(&directory());
        let stats: Vec<_> = view
            .stats
            .iter()
            .map(|s| (s.label.as_str(), s.value.as_str(), s.caption.as_str()))
            .collect();
        assert_eq!(
            stats,
            [
                ("Frações", "3", "1 sem proprietário"),
                ("Proprietários", "2", "Membros do condomínio"),
                // Pedro Costa lives in two units: both count, he counts once as a person.
                (
                    "Frações com residente",
                    "2/3",
                    "Residente indicado, só a título informativo"
                ),
            ]
        );
        assert_eq!(view.units.len(), 3);
        let first = &view.units[0];
        assert_eq!(
            (
                first.owner.as_str(),
                first.owner_initials.as_str(),
                first.resident.as_str()
            ),
            ("Maria Silva", "MS", "Pedro Costa")
        );
        // Owner and resident are different people in a rented flat.
        assert_eq!(view.units[1].owner, "Rui Carvalho");
        assert_eq!(view.units[1].resident, "Pedro Costa");
        // Nobody recorded.
        assert_eq!(
            (&view.units[2].owner[..], &view.units[2].resident[..]),
            ("", "")
        );
        assert_eq!(view.activity[0].text, "Saída de proprietário · 1.o Esq.");
        assert!(view.activity[0].departed);
        assert_eq!(view.activity[0].date, "01/05/2025");
        assert_eq!(view.activity[1].text, "Novo residente · R/C Dir.");
    }

    #[test]
    fn a_unit_still_shows_every_active_owner_a_legacy_database_may_hold() {
        let mut dir = directory();
        dir.relationships.push(rel(
            6,
            (1, "1.o Esq."),
            (5, "Co-proprietária"),
            UnitRole::Owner,
            d(2025, 1, 1),
            None,
        ));
        assert_eq!(
            unit_view(&dir, &dir.units[0]).owner,
            "Maria Silva, Co-proprietária"
        );
    }

    #[test]
    fn an_empty_condominium_has_zeroes_not_panics() {
        let view = overview(&Directory {
            units: vec![],
            people: vec![],
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
                UnitRole::Resident,
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
                people: vec![],
                relationships: vec![],
            },
            "",
        );
        assert_eq!(empty.empty_title, "Ainda não existem frações.");
    }

    #[test]
    fn people_split_into_owner_and_resident_tabs_with_history() {
        let dir = directory();
        let owners = people_page(&dir, "", false, UnitRole::Owner);
        assert_eq!(
            (owners.active_count, owners.history_count),
            (2, 1),
            "tab counts"
        );
        assert_eq!((owners.owners_count, owners.residents_count), (2, 2));
        assert_eq!(
            owners
                .rows
                .iter()
                .map(|r| (r.name.as_str(), r.role.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Maria Silva", "Proprietário"),
                ("Rui Carvalho", "Proprietário")
            ]
        );
        assert_eq!(owners.subtitle, "2 proprietários ativos");

        let residents = people_page(&dir, "", false, UnitRole::Resident);
        assert_eq!((residents.active_count, residents.history_count), (2, 0));
        assert_eq!(
            residents
                .rows
                .iter()
                .map(|r| (r.name.as_str(), r.role.as_str()))
                .collect::<Vec<_>>(),
            [("Pedro Costa", "Residente"), ("Pedro Costa", "Residente")]
        );

        let history = people_page(&dir, "", true, UnitRole::Owner);
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
        assert!(people_page(&dir, "", true, UnitRole::Resident)
            .rows
            .is_empty());
    }

    #[test]
    fn people_search_matches_name_unit_and_email_without_accents() {
        let mut dir = directory();
        dir.relationships[2].email = Some("maria@example.test".into());
        let names = |q, h, role| {
            people_page(&dir, q, h, role)
                .rows
                .into_iter()
                .map(|r| r.name)
                .collect::<Vec<_>>()
        };
        let owner = UnitRole::Owner;
        assert_eq!(names("MARIA", false, owner), ["Maria Silva"]);
        assert_eq!(names("example", false, owner), ["Maria Silva"]);
        assert_eq!(names("r/c", false, owner), ["Rui Carvalho"]);
        assert_eq!(names("r/c", false, UnitRole::Resident), ["Pedro Costa"]);
        assert_eq!(names("alvaro", true, owner), ["Álvaro Maria"]);
        // History only shows in history, and owners never show in the residents tab.
        assert_eq!(names("alvaro", false, owner), Vec::<String>::new());
        assert_eq!(
            names("maria", false, UnitRole::Resident),
            Vec::<String>::new()
        );
        let page = people_page(&dir, "zzz", true, owner);
        assert_eq!(page.empty_title, "Nenhuma pessoa encontrada.");
        let nobody = Directory {
            relationships: vec![],
            ..dir.clone()
        };
        assert_eq!(
            people_page(&nobody, "", true, owner).empty_title,
            "Sem saídas registadas."
        );
        assert_eq!(
            people_page(&nobody, "", false, owner).empty_title,
            "Ainda não existem proprietários registados."
        );
        assert_eq!(
            people_page(&nobody, "", false, UnitRole::Resident).empty_title,
            "Ainda não existem residentes registados."
        );
    }

    #[test]
    fn contact_prefers_email_then_phone() {
        let mut r = rel(
            1,
            (1, "A"),
            (1, "Ana"),
            UnitRole::Owner,
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
