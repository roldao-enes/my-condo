//! Email announcements: drafts and the recipient snapshot taken when one is queued.
//! Ported from `app/mail/application/announcement_service.ts`.

use chrono::NaiveDateTime;
use condo_core::locale::collate_pt;
use condo_core::recipients::{build_recipients, RecipientRow};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::audit_events;
use crate::error::{text, ServiceError, ServiceResult};
use crate::write_tx;

pub const MAX_BODY_CHARS: usize = 10_000;

pub fn create_draft(
    conn: &mut Connection,
    actor_id: i64,
    subject: &str,
    body: &str,
) -> ServiceResult<i64> {
    let subject = text(
        subject,
        1,
        200,
        "O assunto deve ter entre 1 e 200 caracteres.",
    )?;
    let body = text(
        body,
        1,
        MAX_BODY_CHARS,
        "A mensagem deve ter entre 1 e 10000 caracteres.",
    )?;
    let tx = write_tx(conn)?;
    tx.execute(
        "INSERT INTO announcements (subject, body, created_by) VALUES (?1, ?2, ?3)",
        params![subject, body, actor_id],
    )?;
    let id = tx.last_insert_rowid();
    audit_events::record(
        &tx,
        actor_id,
        "announcement.created",
        "announcement",
        id,
        None,
    )?;
    tx.commit()?;
    Ok(id)
}

/// Snapshots the recipients (people with an active relationship and a valid email, one per
/// distinct address) into email jobs and locks the announcement. Returns how many were queued.
/// People who move in or out afterwards do not change an already queued announcement.
pub fn queue(
    conn: &mut Connection,
    actor_id: i64,
    announcement_id: i64,
    unit_id: Option<i64>,
) -> ServiceResult<usize> {
    let tx = write_tx(conn)?;
    let status: Option<String> = tx
        .query_row(
            "SELECT status FROM announcements WHERE id = ?1",
            [announcement_id],
            |row| row.get(0),
        )
        .optional()?;
    match status.as_deref() {
        None => return Err(ServiceError::NotFound("Comunicado não encontrado.".into())),
        Some("draft") => {}
        Some(_) => {
            return Err(ServiceError::Conflict(
                "Este comunicado já foi colocado em fila.".into(),
            ))
        }
    }
    if let Some(unit_id) = unit_id {
        let exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM units WHERE id = ?1)",
            [unit_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(ServiceError::NotFound("Fração não encontrada.".into()));
        }
    }

    // `?1 IS NULL OR ...` lets one statement serve both "all units" and "one unit".
    let mut rows: Vec<RecipientRow> = tx
        .prepare(
            "SELECT r.id, r.name, r.email, u.label
               FROM occupancies o
               JOIN people r ON r.id = o.person_id
               JOIN units u ON u.id = o.unit_id
              WHERE o.role = 'owner' AND o.end_date IS NULL
                AND (?1 IS NULL OR o.unit_id = ?1)",
        )?
        .query_map([unit_id], |row| {
            Ok(RecipientRow {
                person_id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                unit_label: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    // Deterministic order (unit, then name); it decides which name a shared address keeps.
    rows.sort_by(|a, b| {
        collate_pt(&a.unit_label, &b.unit_label).then_with(|| collate_pt(&a.name, &b.name))
    });
    let recipients = build_recipients(&rows);
    if recipients.is_empty() {
        return Err(ServiceError::Rejected(
            "Não existem destinatários com email para este comunicado.".into(),
        ));
    }

    {
        let mut insert = tx.prepare(
            "INSERT INTO email_jobs
               (announcement_id, person_id, recipient_name, recipient_email, unit_labels)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for recipient in &recipients {
            insert.execute(params![
                announcement_id,
                recipient.person_id,
                recipient.name,
                recipient.email,
                recipient.unit_labels.join(", ")
            ])?;
        }
    }
    tx.execute(
        "UPDATE announcements
            SET status = 'queued', queued_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        [announcement_id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "announcement.queued",
        "announcement",
        announcement_id,
        Some(&json!({ "recipients": recipients.len(), "unitId": unit_id })),
    )?;
    tx.commit()?;
    Ok(recipients.len())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnouncementSummary {
    pub id: i64,
    pub subject: String,
    pub status: String,
    pub created_at: String,
    pub total: i64,
    pub sent: i64,
    pub failed: i64,
}

/// Newest first, with delivery counts.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<AnnouncementSummary>> {
    list_in(conn, None)
}

/// Like [`list`], but only the announcements created in `year`.
pub fn list_in_year(conn: &Connection, year: i32) -> rusqlite::Result<Vec<AnnouncementSummary>> {
    list_in(conn, Some(year))
}

fn list_in(conn: &Connection, year: Option<i32>) -> rusqlite::Result<Vec<AnnouncementSummary>> {
    let year = year.map(|y| format!("{y:04}"));
    conn.prepare(
        "SELECT a.id, a.subject, a.status, a.created_at,
                count(j.id),
                count(j.id) FILTER (WHERE j.status = 'sent'),
                count(j.id) FILTER (WHERE j.status = 'failed')
           FROM announcements a
           LEFT JOIN email_jobs j ON j.announcement_id = a.id
          WHERE ?1 IS NULL OR substr(a.created_at, 1, 4) = ?1
          GROUP BY a.id
          ORDER BY a.id DESC",
    )?
    .query_map([year], |row| {
        Ok(AnnouncementSummary {
            id: row.get(0)?,
            subject: row.get(1)?,
            status: row.get(2)?,
            created_at: row.get(3)?,
            total: row.get(4)?,
            sent: row.get(5)?,
            failed: row.get(6)?,
        })
    })?
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub id: i64,
    pub subject: String,
    pub body: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub unit_labels: String,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnouncementDetail {
    pub announcement: Announcement,
    /// Sorted by recipient name.
    pub jobs: Vec<JobView>,
    /// For the "limit to one unit" picker: (id, label).
    pub units: Vec<(i64, String)>,
}

pub fn show(conn: &Connection, announcement_id: i64) -> ServiceResult<AnnouncementDetail> {
    let announcement = conn
        .query_row(
            "SELECT id, subject, body, status, created_at FROM announcements WHERE id = ?1",
            [announcement_id],
            |row| {
                Ok(Announcement {
                    id: row.get(0)?,
                    subject: row.get(1)?,
                    body: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| ServiceError::NotFound("Comunicado não encontrado.".into()))?;

    let mut jobs: Vec<JobView> = conn
        .prepare(
            "SELECT id, recipient_name, recipient_email, unit_labels, status, attempts,
                    last_error, sent_at
               FROM email_jobs WHERE announcement_id = ?1",
        )?
        .query_map([announcement_id], |row| {
            Ok(JobView {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                unit_labels: row.get(3)?,
                status: row.get(4)?,
                attempts: row.get(5)?,
                last_error: row.get(6)?,
                sent_at: row.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    jobs.sort_by(|a, b| collate_pt(&a.name, &b.name).then(a.id.cmp(&b.id)));

    let mut units: Vec<(i64, String)> = conn
        .prepare("SELECT id, label FROM units")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    units.sort_by(|a, b| collate_pt(&a.1, &b.1));

    Ok(AnnouncementDetail {
        announcement,
        jobs,
        units,
    })
}

/// Parses the stored `created_at` instant for display.
pub fn parse_timestamp(value: &str) -> Option<NaiveDateTime> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.naive_utc())
}
