//! Finding out that a newer version exists. Notify only: nothing is downloaded or installed.
//!
//! The release pipeline publishes `latest.json` and `latest.json.sig` (see `docs/updates.md`).
//! [`check`] fetches both, accepts the manifest only if its signature verifies against a public
//! key compiled into the app, and compares versions. The request carries nothing about the
//! person or the condominium: it is a plain GET of a public file.
//!
//! Whether to check, when it last did and which version the person dismissed live in the
//! `app_settings` table, so they survive restarts and travel with a backup.

use std::io::Read;
use std::time::Duration;

use chrono::{DateTime, Utc};
use condo_db::rusqlite::Connection;
use condo_db::{settings, timestamp};
use condo_update::signing::embedded_keys;
use condo_update::{assess, current_platform, open, UpdateError, UpdateStatus};

/// Where the newest *tagged* release's manifest lives. GitHub's `latest` skips pre-releases, so
/// the per-merge builds are never announced. Overridable at build time for forks.
pub const MANIFEST_URL: &str = match option_env!("CONDO_UPDATE_URL") {
    Some(url) => url,
    None => "https://github.com/roldao-enes/my-condo/releases/latest/download/latest.json",
};

/// How often the app checks by itself.
pub const CHECK_EVERY: chrono::Duration = chrono::Duration::hours(24);

const ENABLED: &str = "updates.enabled";
const LAST_CHECK: &str = "updates.last_check";
const DISMISSED: &str = "updates.dismissed_version";

/// A manifest is a few hundred bytes; anything far larger is not ours.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 4 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("Esta versão não tem a verificação de atualizações configurada.")]
    NotConfigured,
    #[error("Não foi possível contactar o servidor de atualizações: {0}")]
    Network(String),
    #[error("As informações de atualização recebidas não são de confiança e foram ignoradas.")]
    Untrusted,
    #[error("As informações de atualização estão num formato que esta versão não entende.")]
    Unreadable,
}

/// Fetches the signed manifest. Behind a trait so tests (and the UI tests) need no network.
pub trait UpdateSource: Send + Sync {
    /// The exact bytes of `latest.json` and the base64 text of `latest.json.sig`.
    fn fetch(&self) -> Result<(Vec<u8>, String), CheckError>;
}

/// Downloads over HTTPS (never plain HTTP, also across redirects), with time and size limits.
pub struct HttpSource {
    url: String,
    https_only: bool,
}

impl HttpSource {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            https_only: true,
        }
    }

    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, CheckError> {
        let net = |e: &dyn std::fmt::Display| CheckError::Network(e.to_string());
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(self.https_only)
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent("meu-condominio-update-check")
            .build()
            .into();
        let mut response = agent.get(url).call().map_err(|e| net(&e))?;
        // `take(limit + 1)` so a body over the limit is detected rather than silently cut.
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| net(&e))?;
        if bytes.len() as u64 > limit {
            return Err(CheckError::Untrusted);
        }
        Ok(bytes)
    }
}

impl UpdateSource for HttpSource {
    fn fetch(&self) -> Result<(Vec<u8>, String), CheckError> {
        let manifest = self.get(&self.url, MAX_MANIFEST_BYTES)?;
        let signature = self.get(&format!("{}.sig", self.url), MAX_SIGNATURE_BYTES)?;
        let signature = String::from_utf8(signature).map_err(|_| CheckError::Untrusted)?;
        Ok((manifest, signature))
    }
}

/// Everything the app needs from the update machinery. Behind a trait so UI tests can decide
/// what "the newest version" is without keys or a network.
pub trait UpdateCheck: Send + Sync {
    /// Whether this build can verify updates at all (a public key was compiled in).
    fn configured(&self) -> bool;
    /// Fetches, verifies and compares. `current` is this build's version (`CARGO_PKG_VERSION`).
    fn check(&self, current: &str) -> Result<UpdateStatus, CheckError>;
}

/// The real thing: the signed manifest from `source`, checked with the compiled-in keys.
pub struct SignedCheck {
    source: Box<dyn UpdateSource>,
}

impl SignedCheck {
    pub fn new(source: Box<dyn UpdateSource>) -> Self {
        Self { source }
    }

    /// Downloads from [`MANIFEST_URL`].
    pub fn from_release_page() -> Self {
        Self::new(Box::new(HttpSource::new(MANIFEST_URL)))
    }
}

impl UpdateCheck for SignedCheck {
    fn configured(&self) -> bool {
        !embedded_keys().is_empty()
    }

    fn check(&self, current: &str) -> Result<UpdateStatus, CheckError> {
        check_with(
            &*self.source,
            &embedded_keys(),
            current,
            &current_platform(),
        )
    }
}

pub fn check_with(
    source: &dyn UpdateSource,
    keys: &[condo_update::signing::VerifyingKey],
    current: &str,
    platform: &str,
) -> Result<UpdateStatus, CheckError> {
    if keys.is_empty() {
        return Err(CheckError::NotConfigured);
    }
    let (bytes, signature) = source.fetch()?;
    let manifest = open(&bytes, &signature, keys).map_err(|e| match e {
        UpdateError::BadSignature | UpdateError::NoKeys => CheckError::Untrusted,
        _ => CheckError::Unreadable,
    })?;
    assess(&manifest, current, platform).map_err(|_| CheckError::Unreadable)
}

// --- Settings -----------------------------------------------------------------------------

/// On unless the person turned it off.
pub fn is_enabled(conn: &Connection) -> bool {
    settings::get(conn, ENABLED).ok().flatten().as_deref() != Some("0")
}

pub fn set_enabled(conn: &Connection, enabled: bool) -> condo_db::rusqlite::Result<()> {
    settings::set(conn, ENABLED, if enabled { "1" } else { "0" })
}

/// Time for the automatic check: enabled, possible, and none made in the last [`CHECK_EVERY`].
pub fn is_due(conn: &Connection, now: DateTime<Utc>, configured: bool) -> bool {
    if !configured || !is_enabled(conn) {
        return false;
    }
    match last_check(conn) {
        Some(last) => now - last >= CHECK_EVERY,
        None => true,
    }
}

pub fn last_check(conn: &Connection) -> Option<DateTime<Utc>> {
    let text = settings::get(conn, LAST_CHECK).ok().flatten()?;
    DateTime::parse_from_rfc3339(&text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Remembers that a check was made, whether it worked or not, so being offline does not mean
/// retrying on every start.
pub fn record_check(conn: &Connection, now: DateTime<Utc>) -> condo_db::rusqlite::Result<()> {
    settings::set(conn, LAST_CHECK, &timestamp(now))
}

/// "Not now" for this version: the banner stays away until a newer one comes out.
pub fn dismiss(conn: &Connection, version: &str) -> condo_db::rusqlite::Result<()> {
    settings::set(conn, DISMISSED, version)
}

/// Whether to show the banner for `status`: a required update always, an optional one unless the
/// person already dismissed that very version.
pub fn should_announce(conn: &Connection, status: &UpdateStatus) -> bool {
    match status {
        UpdateStatus::UpToDate => false,
        UpdateStatus::Available {
            version, required, ..
        } => {
            *required
                || settings::get(conn, DISMISSED).ok().flatten().as_deref()
                    != Some(&version.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use chrono::TimeZone;
    use condo_db::open_in_memory;
    use condo_update::manifest::{Asset, Manifest, SCHEMA};
    use condo_update::release::to_bytes;
    use condo_update::signing::{generate_keypair, parse_public_keys, sign, VerifyingKey};

    use super::*;

    struct Fixed(Result<(Vec<u8>, String), String>);
    impl UpdateSource for Fixed {
        fn fetch(&self) -> Result<(Vec<u8>, String), CheckError> {
            self.0.clone().map_err(CheckError::Network)
        }
    }

    fn manifest(version: &str, minimum: Option<&str>) -> Vec<u8> {
        to_bytes(&Manifest {
            schema: SCHEMA,
            version: version.to_owned(),
            published: "2026-12-01T00:00:00Z".to_owned(),
            notes_url: Some("https://example.test/notes".to_owned()),
            minimum_version: minimum.map(str::to_owned),
            assets: vec![Asset {
                platform: "macos".to_owned(),
                name: "a.dmg".to_owned(),
                url: "https://example.test/a.dmg".to_owned(),
                sha256: "0".repeat(64),
                size: 1,
            }],
        })
    }

    fn trusted() -> (String, Vec<VerifyingKey>) {
        let (private, public) = generate_keypair();
        (private, parse_public_keys(&public).unwrap())
    }

    #[test]
    fn a_correctly_signed_newer_version_is_announced() {
        let (private, keys) = trusted();
        let bytes = manifest("0.2.0", None);
        let signature = sign(&private, &bytes).unwrap();
        let status = check_with(&Fixed(Ok((bytes, signature))), &keys, "0.1.0", "macos").unwrap();
        assert!(matches!(
            status,
            UpdateStatus::Available {
                required: false,
                asset: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn the_same_or_an_older_version_is_not() {
        let (private, keys) = trusted();
        for published in ["0.1.0", "0.0.9"] {
            let bytes = manifest(published, None);
            let signature = sign(&private, &bytes).unwrap();
            let status =
                check_with(&Fixed(Ok((bytes, signature))), &keys, "0.1.0", "macos").unwrap();
            assert_eq!(status, UpdateStatus::UpToDate);
        }
    }

    #[test]
    fn an_unsigned_forged_or_tampered_manifest_is_never_believed() {
        let (private, keys) = trusted();
        let (attacker, _) = generate_keypair();
        let bytes = manifest("9.9.9", None);
        let genuine = sign(&private, &manifest("0.2.0", None)).unwrap();
        for signature in [
            sign(&attacker, &bytes).unwrap(), // someone else's key
            genuine,                          // a real signature, for different content
            String::new(),                    // none at all
        ] {
            let result = check_with(
                &Fixed(Ok((bytes.clone(), signature))),
                &keys,
                "0.1.0",
                "macos",
            );
            assert!(matches!(result, Err(CheckError::Untrusted)), "{result:?}");
        }
    }

    #[test]
    fn a_build_without_a_key_does_not_check_at_all() {
        let result = check_with(&Fixed(Err("never reached".into())), &[], "0.1.0", "macos");
        assert!(matches!(result, Err(CheckError::NotConfigured)));
    }

    #[test]
    fn a_network_failure_is_reported_as_one() {
        let (_, keys) = trusted();
        let result = check_with(&Fixed(Err("offline".into())), &keys, "0.1.0", "macos");
        assert!(matches!(result, Err(CheckError::Network(_))));
    }

    #[test]
    fn checking_is_on_by_default_daily_and_can_be_turned_off() {
        let conn = open_in_memory().unwrap();
        let t0 = chrono::Utc.with_ymd_and_hms(2026, 12, 1, 9, 0, 0).unwrap();
        assert!(is_enabled(&conn));
        assert_eq!(last_check(&conn), None);
        record_check(&conn, t0).unwrap();
        assert_eq!(last_check(&conn), Some(t0));
        set_enabled(&conn, false).unwrap();
        assert!(!is_enabled(&conn));
        set_enabled(&conn, true).unwrap();
        assert!(is_enabled(&conn));
    }

    #[test]
    fn the_automatic_check_is_due_once_a_day_when_possible_and_wanted() {
        let conn = open_in_memory().unwrap();
        let t0 = chrono::Utc.with_ymd_and_hms(2026, 12, 1, 9, 0, 0).unwrap();
        assert!(is_due(&conn, t0, true), "never checked");
        assert!(!is_due(&conn, t0, false), "no key compiled in");
        record_check(&conn, t0).unwrap();
        assert!(!is_due(&conn, t0 + chrono::Duration::hours(23), true));
        assert!(is_due(&conn, t0 + chrono::Duration::hours(24), true));
        set_enabled(&conn, false).unwrap();
        assert!(!is_due(&conn, t0 + chrono::Duration::days(30), true));
    }

    #[test]
    fn the_banner_respects_a_dismissed_version_unless_the_update_is_required() {
        let conn = open_in_memory().unwrap();
        let available = |version: &str, required| UpdateStatus::Available {
            version: version.parse().unwrap(),
            notes_url: None,
            asset: None,
            required,
        };
        assert!(!should_announce(&conn, &UpdateStatus::UpToDate));
        assert!(should_announce(&conn, &available("0.2.0", false)));
        dismiss(&conn, "0.2.0").unwrap();
        assert!(!should_announce(&conn, &available("0.2.0", false)));
        assert!(should_announce(&conn, &available("0.2.0", true)));
        assert!(should_announce(&conn, &available("0.3.0", false)));
    }

    /// A one-shot HTTP server on localhost answering every request with `response`.
    fn serve(response: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(&response);
            }
        });
        format!("http://{address}/latest.json")
    }

    fn http(status: &str, body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn local(url: &str) -> HttpSource {
        HttpSource {
            url: url.to_owned(),
            https_only: false,
        }
    }

    #[test]
    fn http_source_downloads_manifest_and_signature_and_enforces_limits() {
        let url = serve(http("200 OK", b"hello"));
        let (bytes, signature) = local(&url).fetch().unwrap();
        assert_eq!((bytes, signature.as_str()), (b"hello".to_vec(), "hello"));

        let missing = serve(http("404 Not Found", b"nope"));
        assert!(matches!(
            local(&missing).fetch(),
            Err(CheckError::Network(_))
        ));

        let huge = serve(http("200 OK", &vec![b'x'; 300 * 1024]));
        assert!(matches!(local(&huge).fetch(), Err(CheckError::Untrusted)));
    }

    #[test]
    fn plain_http_is_refused_by_the_real_source() {
        let url = serve(http("200 OK", b"hello"));
        assert!(matches!(
            HttpSource::new(&url).fetch(),
            Err(CheckError::Network(_))
        ));
    }

    #[test]
    fn the_manifest_address_is_https() {
        assert!(MANIFEST_URL.starts_with("https://"));
    }
}
