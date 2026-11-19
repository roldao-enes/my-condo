//! Ported from tests/functional/email_sender.spec.ts.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use condo_core::occupancy::ResidentRole;
use condo_core::retry::MAX_ATTEMPTS;
use condo_db::residency::{self, Onboarding, ResidentRef};
use condo_db::rusqlite::Connection;
use condo_db::{announcements, open_in_memory, timestamp, users};
use condo_mail::{Clock, EmailSender, MailError, MailTransport, OutgoingEmail, SendSummary};

/// Records what it was asked to send and can be told to fail.
#[derive(Clone, Default)]
struct FakeTransport {
    sent: Arc<Mutex<Vec<OutgoingEmail>>>,
    failing: Arc<Mutex<bool>>,
}

impl FakeTransport {
    fn set_failing(&self, failing: bool) {
        *self.failing.lock().unwrap() = failing;
    }
    fn sent(&self) -> Vec<OutgoingEmail> {
        self.sent.lock().unwrap().clone()
    }
}

impl MailTransport for FakeTransport {
    fn send(&self, message: &OutgoingEmail) -> Result<(), MailError> {
        if *self.failing.lock().unwrap() {
            return Err(MailError("connect ECONNREFUSED".into()));
        }
        self.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
}

/// A clock the test moves by hand.
#[derive(Clone)]
struct TestClock(Arc<Mutex<DateTime<Utc>>>);

impl TestClock {
    /// Starts one second in the future so jobs created "now" are already due.
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Utc::now() + Duration::seconds(1))))
    }
    fn get(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
    fn clock(&self) -> Clock {
        let inner = Arc::clone(&self.0);
        Arc::new(move || *inner.lock().unwrap())
    }
}

fn sender(transport: &FakeTransport, clock: &TestClock) -> EmailSender<FakeTransport> {
    EmailSender::new(transport.clone()).with_clock(clock.clock())
}

fn done(sent: usize, retrying: usize, failed: usize) -> SendSummary {
    SendSummary {
        sent,
        retrying,
        failed,
    }
}

fn setup() -> (Connection, i64, i64) {
    let mut conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, Some("Test Admin"), "admin@example.test", "hash").unwrap();
    let unit = residency::create_unit(&mut conn, actor, "1A", "1").unwrap();
    (conn, actor, unit)
}

fn add_resident(
    conn: &mut Connection,
    actor: i64,
    unit: i64,
    name: &str,
    email: &str,
    role: ResidentRole,
) {
    residency::onboard(
        conn,
        actor,
        &Onboarding {
            unit_id: unit,
            resident: ResidentRef::New {
                name: name.into(),
                email: Some(email.into()),
                phone: None,
            },
            role,
            start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
        },
        NaiveDate::from_ymd_opt(2025, 6, 15).unwrap(),
    )
    .unwrap();
}

/// One resident, one queued announcement; returns the job id.
fn queue_one(conn: &mut Connection, actor: i64, unit: i64) -> i64 {
    add_resident(
        conn,
        actor,
        unit,
        "Ana",
        "ana@example.test",
        ResidentRole::Owner,
    );
    let id = announcements::create_draft(conn, actor, "Hi", "Body text").unwrap();
    announcements::queue(conn, actor, id, None).unwrap();
    conn.query_row("SELECT id FROM email_jobs", [], |r| r.get(0))
        .unwrap()
}

struct Job {
    status: String,
    attempts: i64,
    last_error: Option<String>,
    next_attempt_at: String,
    sent_at: Option<String>,
}

fn job(conn: &Connection, id: i64) -> Job {
    conn.query_row(
        "SELECT status, attempts, last_error, next_attempt_at, sent_at FROM email_jobs WHERE id = ?1",
        [id],
        |r| Ok(Job { status: r.get(0)?, attempts: r.get(1)?, last_error: r.get(2)?, next_attempt_at: r.get(3)?, sent_at: r.get(4)? }),
    )
    .unwrap()
}

fn outcomes(conn: &Connection, id: i64) -> Vec<String> {
    conn.prepare("SELECT outcome FROM email_attempts WHERE email_job_id = ?1 ORDER BY id")
        .unwrap()
        .query_map([id], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn sends_a_due_job_once_and_records_the_attempt() {
    let (mut conn, actor, unit) = setup();
    let job_id = queue_one(&mut conn, actor, unit);
    let (transport, clock) = (FakeTransport::default(), TestClock::new());
    let sender = sender(&transport, &clock);

    assert_eq!(sender.count_due(&conn).unwrap(), 1);
    assert_eq!(sender.run(&mut conn, 10).unwrap(), done(1, 0, 0));
    assert_eq!(sender.run(&mut conn, 10).unwrap(), done(0, 0, 0));

    let sent = transport.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (sent[0].to_name.as_str(), sent[0].to_address.as_str()),
        ("Ana", "ana@example.test")
    );
    assert_eq!(
        (sent[0].subject.as_str(), sent[0].text.as_str()),
        ("Hi", "Body text")
    );
    let row = job(&conn, job_id);
    assert_eq!((row.status.as_str(), row.attempts), ("sent", 1));
    assert!(row.sent_at.is_some());
    assert_eq!(outcomes(&conn, job_id), ["ok"]);
}

#[test]
fn schedules_a_retry_after_a_failure_and_skips_the_job_until_it_is_due() {
    let (mut conn, actor, unit) = setup();
    let job_id = queue_one(&mut conn, actor, unit);
    let (transport, clock) = (FakeTransport::default(), TestClock::new());
    transport.set_failing(true);
    let sender = sender(&transport, &clock);
    let start = clock.get();

    assert_eq!(sender.run(&mut conn, 10).unwrap(), done(0, 1, 0));
    let row = job(&conn, job_id);
    assert_eq!((row.status.as_str(), row.attempts), ("pending", 1));
    assert!(row.last_error.unwrap().contains("ECONNREFUSED"));
    assert_eq!(row.next_attempt_at, timestamp(start + Duration::minutes(5)));

    // Not due yet: nothing is attempted.
    assert_eq!(sender.run(&mut conn, 10).unwrap(), done(0, 0, 0));
    assert_eq!(sender.count_due(&conn).unwrap(), 0);

    transport.set_failing(false);
    clock.advance(Duration::minutes(5));
    assert_eq!(sender.run(&mut conn, 10).unwrap(), done(1, 0, 0));
    let row = job(&conn, job_id);
    assert_eq!(
        (row.status.as_str(), row.attempts, row.last_error),
        ("sent", 2, None)
    );
    assert_eq!(outcomes(&conn, job_id), ["error", "ok"]);
}

#[test]
fn gives_up_after_the_maximum_number_of_attempts() {
    let (mut conn, actor, unit) = setup();
    let job_id = queue_one(&mut conn, actor, unit);
    let (transport, clock) = (FakeTransport::default(), TestClock::new());
    transport.set_failing(true);
    let sender = sender(&transport, &clock);

    let mut last = SendSummary::default();
    for _ in 0..MAX_ATTEMPTS {
        last = sender.run(&mut conn, 10).unwrap();
        clock.advance(Duration::days(1));
    }
    assert_eq!(last, done(0, 0, 1));
    let row = job(&conn, job_id);
    assert_eq!(
        (row.status.as_str(), row.attempts),
        ("failed", i64::from(MAX_ATTEMPTS))
    );
    assert_eq!(outcomes(&conn, job_id).len(), MAX_ATTEMPTS as usize);
    // A failed job is never picked up again.
    assert_eq!(sender.run(&mut conn, 10).unwrap(), done(0, 0, 0));
}

#[test]
fn reclaims_a_job_whose_earlier_run_died_after_the_lease_expired() {
    let (mut conn, actor, unit) = setup();
    let job_id = queue_one(&mut conn, actor, unit);
    let (transport, clock) = (FakeTransport::default(), TestClock::new());
    let start = clock.get();
    // Simulate a run that claimed the job and then died: leased for 10 minutes.
    conn.execute(
        "UPDATE email_jobs SET status = 'sending', next_attempt_at = ?1 WHERE id = ?2",
        (timestamp(start + Duration::minutes(10)), job_id),
    )
    .unwrap();

    assert_eq!(
        sender(&transport, &clock).run(&mut conn, 10).unwrap(),
        done(0, 0, 0)
    );
    clock.advance(Duration::minutes(11));
    assert_eq!(
        sender(&transport, &clock).run(&mut conn, 10).unwrap(),
        done(1, 0, 0)
    );
    assert_eq!(transport.sent().len(), 1);
}

#[test]
fn claiming_leases_the_job_so_a_second_runner_does_not_send_it_again() {
    let (mut conn, actor, unit) = setup();
    queue_one(&mut conn, actor, unit);
    let clock = TestClock::new();
    let claimed = condo_db::email_queue::claim_next(&mut conn, clock.get()).unwrap();
    assert!(claimed.is_some());
    // While leased, it is not due for anyone else.
    assert!(condo_db::email_queue::claim_next(&mut conn, clock.get())
        .unwrap()
        .is_none());
    assert_eq!(
        condo_db::email_queue::count_due(&conn, clock.get()).unwrap(),
        0
    );
}

#[test]
fn honours_the_per_run_limit() {
    let (mut conn, actor, unit) = setup();
    for email in ["a@example.test", "b@example.test", "c@example.test"] {
        add_resident(&mut conn, actor, unit, email, email, ResidentRole::Occupant);
    }
    let id = announcements::create_draft(&mut conn, actor, "Hi", "Body").unwrap();
    announcements::queue(&mut conn, actor, id, None).unwrap();
    let (transport, clock) = (FakeTransport::default(), TestClock::new());
    let sender = sender(&transport, &clock);

    assert_eq!(sender.run(&mut conn, 2).unwrap(), done(2, 0, 0));
    assert_eq!(sender.count_due(&conn).unwrap(), 1);
}

#[test]
fn long_error_messages_are_truncated_before_they_are_stored() {
    struct Verbose;
    impl MailTransport for Verbose {
        fn send(&self, _: &OutgoingEmail) -> Result<(), MailError> {
            Err(MailError("é".repeat(2_000)))
        }
    }
    let (mut conn, actor, unit) = setup();
    let job_id = queue_one(&mut conn, actor, unit);
    let clock = TestClock::new();
    EmailSender::new(Verbose)
        .with_clock(clock.clock())
        .run(&mut conn, 1)
        .unwrap();
    assert_eq!(job(&conn, job_id).last_error.unwrap().chars().count(), 500);
}
