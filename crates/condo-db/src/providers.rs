//! Providers of services to the condominium (electricity, water, cleaning, bank, insurance), the
//! people to contact there, and the names they appear under in the bank statement.
//!
//! Those names are ordinary finance tags that carry a `provider_id`, so importing a statement
//! classifies a provider's movements through the same matching as every other tag. Removing a
//! provider keeps its tags: how movements are classified does not change behind anyone's back.

use std::collections::HashMap;

use condo_core::locale::collate_pt;
use condo_core::providers::{check_email, check_phone, format_iban, normalize_iban, ProviderKind};
use condo_core::tags::{normalize_name, MatchType, TagCategory};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::audit_events;
use crate::error::{text, ServiceError, ServiceResult};
use crate::finance::{write_tag, TagInput};
use crate::write_tx;

/// What a person types about a provider. Everything but the kind and the name may be blank.
#[derive(Debug, Clone)]
pub struct ProviderInput {
    pub kind: ProviderKind,
    pub name: String,
    pub contact_name: String,
    pub email: String,
    pub phone: String,
    pub iban: String,
    pub note: String,
}

/// What a person types about one of a provider's names in the bank statement.
#[derive(Debug, Clone)]
pub struct ProviderTagInput {
    /// Free text; it is normalized before it is stored or matched.
    pub pattern: String,
    pub match_type: MatchType,
    /// Provider movements are expenses: recurring or extraordinary.
    pub category: TagCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTag {
    pub id: i64,
    /// Normalized.
    pub pattern: String,
    pub match_type: MatchType,
    pub category: TagCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    pub id: i64,
    pub kind: ProviderKind,
    pub name: String,
    pub contact_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    /// Without spaces; see [`format_iban`] to show it.
    pub iban: Option<String>,
    pub note: Option<String>,
    pub tags: Vec<ProviderTag>,
}

const KIND_TAKEN: &str = "Já existe um fornecedor deste tipo. Edite-o ou escolha outro tipo.";
const NOT_FOUND: &str = "Fornecedor não encontrado.";

struct Checked {
    kind: ProviderKind,
    name: String,
    contact_name: Option<String>,
    email: Option<String>,
    phone: Option<String>,
    iban: Option<String>,
    note: Option<String>,
}

/// Trims, and turns blank into `None`.
fn optional(value: &str, max: usize, message: &str) -> ServiceResult<Option<String>> {
    let value = text(value, 0, max, message)?;
    Ok(Some(value).filter(|v| !v.is_empty()))
}

fn check(input: &ProviderInput) -> ServiceResult<Checked> {
    let name = text(
        &input.name,
        1,
        160,
        "O nome do fornecedor deve ter entre 1 e 160 caracteres.",
    )?;
    let contact_name = optional(
        &input.contact_name,
        160,
        "O nome do contacto não pode ter mais de 160 caracteres.",
    )?;
    let email = optional(
        &input.email,
        254,
        "O email não pode ter mais de 254 caracteres.",
    )?;
    if let Some(email) = &email {
        check_email(email)?;
    }
    let phone = optional(
        &input.phone,
        40,
        "O telefone não pode ter mais de 40 caracteres.",
    )?;
    if let Some(phone) = &phone {
        check_phone(phone)?;
    }
    let iban = match input.iban.trim() {
        "" => None,
        iban => Some(normalize_iban(iban)?),
    };
    let note = optional(
        &input.note,
        2000,
        "A nota não pode ter mais de 2000 caracteres.",
    )?;
    Ok(Checked {
        kind: input.kind,
        name,
        contact_name,
        email,
        phone,
        iban,
        note,
    })
}

fn kind_taken(
    tx: &rusqlite::Transaction,
    kind: ProviderKind,
    except: Option<i64>,
) -> rusqlite::Result<bool> {
    if !kind.is_unique() {
        return Ok(false);
    }
    tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM providers WHERE kind = ?1 AND id IS NOT ?2)",
        params![kind.as_str(), except],
        |row| row.get(0),
    )
}

fn audit_details(provider: &Checked) -> serde_json::Value {
    // The IBAN and contact details are deliberately left out of the audit trail.
    json!({ "kind": provider.kind.as_str(), "name": provider.name })
}

pub fn create(conn: &mut Connection, actor_id: i64, input: &ProviderInput) -> ServiceResult<i64> {
    let provider = check(input)?;
    let tx = write_tx(conn)?;
    if kind_taken(&tx, provider.kind, None)? {
        return Err(ServiceError::Rejected(KIND_TAKEN.into()));
    }
    tx.execute(
        "INSERT INTO providers (kind, name, contact_name, email, phone, iban, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            provider.kind.as_str(),
            provider.name,
            provider.contact_name,
            provider.email,
            provider.phone,
            provider.iban,
            provider.note
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit_events::record(
        &tx,
        actor_id,
        "provider.created",
        "provider",
        id,
        Some(&audit_details(&provider)),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Saves the provider's details. Its tags are named after it, so they follow a rename.
pub fn update(
    conn: &mut Connection,
    actor_id: i64,
    id: i64,
    input: &ProviderInput,
) -> ServiceResult<()> {
    let provider = check(input)?;
    let tx = write_tx(conn)?;
    if kind_taken(&tx, provider.kind, Some(id))? {
        return Err(ServiceError::Rejected(KIND_TAKEN.into()));
    }
    let changed = tx.execute(
        "UPDATE providers
            SET kind = ?1, name = ?2, contact_name = ?3, email = ?4, phone = ?5, iban = ?6,
                note = ?7
          WHERE id = ?8",
        params![
            provider.kind.as_str(),
            provider.name,
            provider.contact_name,
            provider.email,
            provider.phone,
            provider.iban,
            provider.note,
            id
        ],
    )?;
    if changed == 0 {
        return Err(ServiceError::NotFound(NOT_FOUND.into()));
    }
    tx.execute(
        "UPDATE finance_tags SET name = ?1 WHERE provider_id = ?2",
        params![provider.name, id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "provider.updated",
        "provider",
        id,
        Some(&audit_details(&provider)),
    )?;
    tx.commit()?;
    Ok(())
}

/// Removes a provider. Its tags stay, as ordinary tags, so past and future movements keep
/// their classification.
pub fn delete(conn: &mut Connection, actor_id: i64, id: i64) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let found: Option<(String, String)> = tx
        .query_row(
            "SELECT kind, name FROM providers WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((kind, name)) = found else {
        return Err(ServiceError::NotFound(NOT_FOUND.into()));
    };
    let kept_tags: i64 = tx.query_row(
        "SELECT count(*) FROM finance_tags WHERE provider_id = ?1",
        [id],
        |row| row.get(0),
    )?;
    tx.execute("DELETE FROM providers WHERE id = ?1", [id])?;
    audit_events::record(
        &tx,
        actor_id,
        "provider.deleted",
        "provider",
        id,
        Some(&json!({ "kind": kind, "name": name, "keptTags": kept_tags })),
    )?;
    tx.commit()?;
    Ok(())
}

/// Creates or updates one of the provider's names in the bank statement. The tag carries the
/// provider's name, so that is how it reads in the movements it classifies.
pub fn save_tag(
    conn: &mut Connection,
    actor_id: i64,
    provider_id: i64,
    input: &ProviderTagInput,
    id: Option<i64>,
) -> ServiceResult<i64> {
    if !matches!(
        input.category,
        TagCategory::Recurring | TagCategory::Extraordinary
    ) {
        return Err(ServiceError::InvalidInput(
            "Os movimentos de um fornecedor são despesas recorrentes ou extraordinárias.".into(),
        ));
    }
    let tx = write_tx(conn)?;
    let name: Option<String> = tx
        .query_row(
            "SELECT name FROM providers WHERE id = ?1",
            [provider_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(name) = name else {
        return Err(ServiceError::NotFound(NOT_FOUND.into()));
    };
    if let Some(id) = id {
        let owner: Option<Option<i64>> = tx
            .query_row(
                "SELECT provider_id FROM finance_tags WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if owner != Some(Some(provider_id)) {
            return Err(ServiceError::NotFound("Etiqueta não encontrada.".into()));
        }
    }
    // Saving over a name another provider already holds would quietly take it from them.
    let pattern = normalize_name(&input.pattern);
    let held_by: Option<Option<i64>> = tx
        .query_row(
            "SELECT provider_id FROM finance_tags
              WHERE pattern = ?1 AND match_type = ?2 AND direction = 'debit' AND id IS NOT ?3",
            params![pattern, input.match_type.as_str(), id],
            |row| row.get(0),
        )
        .optional()?;
    if matches!(held_by, Some(Some(other)) if other != provider_id) {
        return Err(ServiceError::Rejected(
            "Este nome no extrato já pertence a outro fornecedor.".into(),
        ));
    }
    let tag_id = write_tag(
        &tx,
        actor_id,
        &TagInput {
            name,
            pattern: input.pattern.clone(),
            match_type: input.match_type,
            category: input.category,
            owner_id: None,
            unit_id: None,
        },
        id,
    )?;
    tx.execute(
        "UPDATE finance_tags SET provider_id = ?1 WHERE id = ?2",
        params![provider_id, tag_id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "provider.tag_saved",
        "provider",
        provider_id,
        Some(&json!({ "tagId": tag_id, "pattern": pattern })),
    )?;
    tx.commit()?;
    Ok(tag_id)
}

/// Every provider with its tags: by kind (in the order of [`ProviderKind::ALL`]), then by name.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Provider>> {
    let mut tags: HashMap<i64, Vec<ProviderTag>> = HashMap::new();
    let mut statement = conn.prepare(
        "SELECT provider_id, id, pattern, match_type, category FROM finance_tags
          WHERE provider_id IS NOT NULL ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        let parse_error = |index: usize, value: &str| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                format!("unknown value {value:?}").into(),
            )
        };
        let (match_type, category): (String, String) = (row.get(3)?, row.get(4)?);
        Ok((
            row.get::<_, i64>(0)?,
            ProviderTag {
                id: row.get(1)?,
                pattern: row.get(2)?,
                match_type: MatchType::parse(&match_type)
                    .ok_or_else(|| parse_error(3, &match_type))?,
                category: TagCategory::parse(&category).ok_or_else(|| parse_error(4, &category))?,
            },
        ))
    })?;
    for row in rows {
        let (provider_id, tag) = row?;
        tags.entry(provider_id).or_default().push(tag);
    }

    let mut providers: Vec<Provider> = conn
        .prepare("SELECT id, kind, name, contact_name, email, phone, iban, note FROM providers")?
        .query_map([], |row| {
            let kind: String = row.get(1)?;
            let id: i64 = row.get(0)?;
            Ok(Provider {
                id,
                kind: ProviderKind::parse(&kind).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        format!("unknown value {kind:?}").into(),
                    )
                })?,
                name: row.get(2)?,
                contact_name: row.get(3)?,
                email: row.get(4)?,
                phone: row.get(5)?,
                iban: row.get(6)?,
                note: row.get(7)?,
                tags: tags.remove(&id).unwrap_or_default(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let rank = |kind: ProviderKind| {
        ProviderKind::ALL
            .iter()
            .position(|k| *k == kind)
            .unwrap_or(usize::MAX)
    };
    providers.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| collate_pt(&a.name, &b.name))
    });
    Ok(providers)
}

impl Provider {
    /// The IBAN in groups of four, or `None`.
    pub fn iban_display(&self) -> Option<String> {
        self.iban.as_deref().map(format_iban)
    }
}
