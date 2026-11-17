//! SMTP delivery through `lettre` with rustls (no OpenSSL, so it behaves the same on
//! Windows, macOS and Linux). Mirrors the web app's nodemailer settings and timeouts.

use std::time::Duration;

use lettre::message::{header::ContentType, Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Address, Transport};

use crate::{MailError, MailTransport, OutgoingEmail};

/// Connection and sender settings (what the settings screen edits).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    /// `true`: TLS from the first byte (port 465). `false`: plain, upgraded with STARTTLS when the
    /// server offers it (ports 587/25, and local test inboxes without TLS).
    pub secure: bool,
    pub user: Option<String>,
    pub password: Option<String>,
    /// The sender, as `address@example.com` or `Name <address@example.com>`.
    pub from: String,
}

impl SmtpSettings {
    /// `secure` defaults to true on port 465 only, like the web app.
    pub fn default_secure(port: u16) -> bool {
        port == 465
    }
}

pub struct SmtpTransport {
    mailer: lettre::SmtpTransport,
    from: Mailbox,
}

impl SmtpTransport {
    pub fn new(settings: &SmtpSettings) -> Result<Self, MailError> {
        let from: Mailbox = settings
            .from
            .parse()
            .map_err(|e| MailError(format!("Remetente invalido: {e}")))?;
        let tls_parameters = TlsParameters::new(settings.host.clone())
            .map_err(|e| MailError(format!("Configuracao TLS invalida: {e}")))?;
        let mut builder = lettre::SmtpTransport::builder_dangerous(&settings.host)
            .port(settings.port)
            .timeout(Some(Duration::from_secs(30)))
            .tls(if settings.secure {
                Tls::Wrapper(tls_parameters)
            } else {
                Tls::Opportunistic(tls_parameters)
            });
        if let Some(user) = settings.user.as_deref().filter(|u| !u.is_empty()) {
            builder = builder.credentials(Credentials::new(
                user.to_owned(),
                settings.password.clone().unwrap_or_default(),
            ));
        }
        Ok(Self {
            mailer: builder.build(),
            from,
        })
    }
}

/// Collapses any run of whitespace or control characters (line breaks included) into one space.
///
/// Names and subjects end up in message headers. `lettre` panics instead of returning an error
/// when a display name contains a line break, and a line break is also how header injection
/// works, so both are removed here rather than trusted to the stored data.
fn single_line(value: &str) -> String {
    value
        .split(|c: char| c.is_control() || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_message(from: &Mailbox, email: &OutgoingEmail) -> Result<Message, MailError> {
    let address: Address = email
        .to_address
        .parse()
        .map_err(|e| MailError(format!("Email do destinatario invalido: {e}")))?;
    let name = Some(single_line(&email.to_name)).filter(|n| !n.is_empty());
    Message::builder()
        .from(from.clone())
        .to(Mailbox::new(name, address))
        .subject(single_line(&email.subject))
        .header(ContentType::TEXT_PLAIN)
        .body(email.text.clone())
        .map_err(|e| MailError(format!("Mensagem invalida: {e}")))
}

impl MailTransport for SmtpTransport {
    fn send(&self, message: &OutgoingEmail) -> Result<(), MailError> {
        let message = build_message(&self.from, message)?;
        self.mailer
            .send(&message)
            .map(|_| ())
            .map_err(|e| MailError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    fn email() -> OutgoingEmail {
        OutgoingEmail {
            to_name: "Ana Silva".into(),
            to_address: "ana@example.test".into(),
            subject: "Reuniao".into(),
            text: "Ola, a reuniao e amanha.".into(),
        }
    }

    /// A one-message SMTP server that does not offer STARTTLS or AUTH, like a local dev inbox.
    /// Returns its port and a handle yielding the commands plus the DATA section it received.
    fn fake_server() -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut seen = Vec::new();
            writer.write_all(b"220 fake ESMTP\r\n").unwrap();
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let line = line.trim_end().to_owned();
                if in_data {
                    if line == "." {
                        in_data = false;
                        writer.write_all(b"250 queued\r\n").unwrap();
                    } else {
                        seen.push(line);
                    }
                    continue;
                }
                let command = line.to_ascii_uppercase();
                seen.push(line);
                let reply: &[u8] = if command.starts_with("EHLO") {
                    b"250 fake\r\n"
                } else if command.starts_with("DATA") {
                    in_data = true;
                    b"354 go ahead\r\n"
                } else if command.starts_with("QUIT") {
                    writer.write_all(b"221 bye\r\n").unwrap();
                    break;
                } else {
                    b"250 OK\r\n"
                };
                writer.write_all(reply).unwrap();
            }
            seen
        });
        (port, handle)
    }

    fn settings(port: u16) -> SmtpSettings {
        SmtpSettings {
            host: "127.0.0.1".into(),
            port,
            secure: false,
            user: None,
            password: None,
            from: "Meu Condominio <condo@example.test>".into(),
        }
    }

    #[test]
    fn delivers_plain_text_over_smtp_to_a_server_without_tls() {
        let (port, server) = fake_server();
        SmtpTransport::new(&settings(port))
            .unwrap()
            .send(&email())
            .unwrap();
        let transcript = server.join().unwrap().join("\n");

        assert!(
            transcript.contains("MAIL FROM:<condo@example.test>"),
            "{transcript}"
        );
        assert!(
            transcript.contains("RCPT TO:<ana@example.test>"),
            "{transcript}"
        );
        assert!(transcript.contains("Subject: Reuniao"), "{transcript}");
        assert!(
            transcript.contains("To: \"Ana Silva\" <ana@example.test>"),
            "{transcript}"
        );
        assert!(
            transcript.contains("Content-Type: text/plain"),
            "{transcript}"
        );
        assert!(
            transcript.contains("Ola, a reuniao e amanha."),
            "{transcript}"
        );
    }

    #[test]
    fn a_refused_connection_is_reported_as_an_error_not_a_panic() {
        // Bind then drop to get a port that is almost certainly closed.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let error = SmtpTransport::new(&settings(port))
            .unwrap()
            .send(&email())
            .unwrap_err();
        assert!(!error.0.is_empty());
    }

    #[test]
    fn rejects_invalid_sender_and_recipient_before_connecting() {
        let bad_from = SmtpSettings {
            from: "not an address".into(),
            ..settings(1)
        };
        assert!(SmtpTransport::new(&bad_from).is_err());
        let from: Mailbox = "condo@example.test".parse().unwrap();
        let bad_to = OutgoingEmail {
            to_address: "nope".into(),
            ..email()
        };
        assert!(build_message(&from, &bad_to).is_err());
    }

    #[test]
    fn line_breaks_in_a_name_or_subject_neither_panic_nor_inject_headers() {
        let from: Mailbox = "condo@example.test".parse().unwrap();
        let hostile = OutgoingEmail {
            to_name: "Eve\r\nBcc: victim@example.test".into(),
            subject: "Hi\r\nBcc: victim@example.test".into(),
            ..email()
        };
        let message = build_message(&from, &hostile).unwrap();
        let raw = String::from_utf8_lossy(&message.formatted()).into_owned();
        assert!(!raw.lines().any(|line| line.starts_with("Bcc:")), "{raw}");
        assert!(
            raw.contains("Subject: Hi Bcc: victim@example.test"),
            "{raw}"
        );
    }

    #[test]
    fn names_that_are_only_whitespace_fall_back_to_the_bare_address() {
        let from: Mailbox = "condo@example.test".parse().unwrap();
        let blank = OutgoingEmail {
            to_name: " \r\n\t ".into(),
            ..email()
        };
        let raw = String::from_utf8_lossy(&build_message(&from, &blank).unwrap().formatted())
            .into_owned();
        assert!(raw.contains("To: ana@example.test"), "{raw}");
    }

    #[test]
    fn single_line_collapses_whitespace_and_controls() {
        assert_eq!(single_line("  a \t b\r\nc\u{0} d  "), "a b c d");
        assert_eq!(single_line("Maria José"), "Maria José");
        assert_eq!(single_line(""), "");
    }

    #[test]
    fn secure_defaults_to_implicit_tls_on_465_only() {
        assert!(SmtpSettings::default_secure(465));
        assert!(!SmtpSettings::default_secure(587));
        assert!(!SmtpSettings::default_secure(1025));
    }
}
