//! What the Comunicados screens show, as plain data. Ports `pages/announcements/*.tsx`.

use chrono::{DateTime, Utc};
use condo_core::locale::date_time;
use condo_db::announcements::{AnnouncementDetail, AnnouncementSummary};

/// A stored instant as Lisbon time, or the raw text if it cannot be read.
fn when(stored: &str) -> String {
    DateTime::parse_from_rfc3339(stored)
        .map(|t| date_time(t.with_timezone(&Utc)))
        .unwrap_or_else(|_| stored.to_owned())
}

pub fn status_label(status: &str) -> &'static str {
    if status == "queued" {
        "Em fila"
    } else {
        "Rascunho"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    pub id: i64,
    pub subject: String,
    pub created: String,
    pub status: &'static str,
    pub queued: bool,
    /// "3/5 · 1 falhados", or "—" for a draft.
    pub delivered: String,
}

pub fn list_rows(items: &[AnnouncementSummary]) -> Vec<ListRow> {
    items
        .iter()
        .map(|a| {
            let queued = a.status == "queued";
            let delivered = if queued {
                let failed = if a.failed > 0 {
                    format!(" · {} falhados", a.failed)
                } else {
                    String::new()
                };
                format!("{}/{}{failed}", a.sent, a.total)
            } else {
                "—".to_owned()
            };
            ListRow {
                id: a.id,
                subject: a.subject.clone(),
                created: when(&a.created_at),
                status: status_label(&a.status),
                queued,
                delivered,
            }
        })
        .collect()
}

pub fn list_subtitle(count: usize) -> String {
    format!("{count} comunicados")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub units: String,
    pub status: &'static str,
    /// "green" | "red" | "neutral"
    pub tone: &'static str,
    pub attempts: i64,
    /// The last delivery error, or "".
    pub error: String,
    pub sent: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailView {
    pub id: i64,
    pub subject: String,
    pub body: String,
    pub status: &'static str,
    pub queued: bool,
    pub created: String,
    pub jobs: Vec<JobRow>,
    /// Recipients not yet delivered or failed for good (pending or sending).
    pub waiting: usize,
    /// Picker rows: the prompt first, then one per unit (ids are in `unit_ids`).
    pub unit_options: Vec<String>,
    pub unit_ids: Vec<i64>,
}

pub fn detail(d: &AnnouncementDetail) -> DetailView {
    let jobs: Vec<JobRow> = d
        .jobs
        .iter()
        .map(|j| {
            let (status, tone) = match j.status.as_str() {
                "sent" => ("Enviado", "green"),
                "failed" => ("Falhou", "red"),
                "sending" => ("A enviar", "neutral"),
                _ => ("Pendente", "neutral"),
            };
            JobRow {
                id: j.id,
                name: j.name.clone(),
                email: j.email.clone(),
                units: j.unit_labels.clone(),
                status,
                tone,
                attempts: j.attempts,
                error: j.last_error.clone().unwrap_or_default(),
                sent: j.sent_at.as_deref().map(when).unwrap_or_default(),
            }
        })
        .collect();
    let waiting = d
        .jobs
        .iter()
        .filter(|j| matches!(j.status.as_str(), "pending" | "sending"))
        .count();
    let mut unit_options = vec!["Todos os residentes ativos com email".to_owned()];
    unit_options.extend(d.units.iter().map(|(_, l)| format!("Apenas a fração {l}")));
    DetailView {
        id: d.announcement.id,
        subject: d.announcement.subject.clone(),
        body: d.announcement.body.clone(),
        status: status_label(&d.announcement.status),
        queued: d.announcement.status == "queued",
        created: when(&d.announcement.created_at),
        jobs,
        waiting,
        unit_options,
        unit_ids: d.units.iter().map(|(id, _)| *id).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use condo_db::announcements::{Announcement, JobView};

    fn summary(status: &str, total: i64, sent: i64, failed: i64) -> AnnouncementSummary {
        AnnouncementSummary {
            id: 1,
            subject: "Obras".into(),
            status: status.into(),
            created_at: "2026-11-17T14:30:00.000Z".into(),
            total,
            sent,
            failed,
        }
    }

    #[test]
    fn list_rows_show_lisbon_time_status_and_delivery() {
        let rows = list_rows(&[
            summary("draft", 0, 0, 0),
            summary("queued", 5, 3, 0),
            summary("queued", 5, 3, 1),
        ]);
        assert_eq!(rows[0].created, "17/11/2026, 14:30");
        assert_eq!(
            (rows[0].status, rows[0].delivered.as_str()),
            ("Rascunho", "—")
        );
        assert_eq!(rows[1].delivered, "3/5");
        assert_eq!(rows[2].delivered, "3/5 · 1 falhados");
        assert!(rows[2].queued && !rows[0].queued);
    }

    #[test]
    fn detail_labels_jobs_counts_waiting_and_offers_units() {
        let job = |id, status: &str, error: Option<&str>| JobView {
            id,
            name: format!("P{id}"),
            email: format!("p{id}@x.test"),
            unit_labels: "1A".into(),
            status: status.into(),
            attempts: 1,
            last_error: error.map(Into::into),
            sent_at: (status == "sent").then(|| "2026-11-17T15:00:00.000Z".into()),
        };
        let d = AnnouncementDetail {
            announcement: Announcement {
                id: 7,
                subject: "S".into(),
                body: "B".into(),
                status: "queued".into(),
                created_at: "2026-07-01T23:30:00.000Z".into(),
            },
            jobs: vec![
                job(1, "sent", None),
                job(2, "failed", Some("recusado")),
                job(3, "pending", None),
                job(4, "sending", None),
            ],
            units: vec![(1, "1A".into()), (2, "1B".into())],
        };
        let v = detail(&d);
        assert_eq!(v.created, "02/07/2026, 00:30");
        let tones: Vec<_> = v.jobs.iter().map(|j| (j.status, j.tone)).collect();
        assert_eq!(
            tones,
            [
                ("Enviado", "green"),
                ("Falhou", "red"),
                ("Pendente", "neutral"),
                ("A enviar", "neutral")
            ]
        );
        assert_eq!(v.jobs[1].error, "recusado");
        assert_eq!(v.jobs[0].sent, "17/11/2026, 15:00");
        assert_eq!(v.waiting, 2);
        assert_eq!(
            v.unit_options,
            [
                "Todos os residentes ativos com email",
                "Apenas a fração 1A",
                "Apenas a fração 1B"
            ]
        );
        assert_eq!(v.unit_ids, [1, 2]);
    }
}
