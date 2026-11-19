import env from '#start/env'
import type { SmtpSettings } from '../app/mail/infrastructure/smtp_transport.js'

/** Returns SMTP settings, or `null` when email delivery has not been configured. */
export function smtpSettings(): SmtpSettings | null {
  const host = env.get('SMTP_HOST')
  const from = env.get('MAIL_FROM')
  if (!host || !from) return null
  const port = env.get('SMTP_PORT', 587)
  return {
    host,
    port,
    secure: env.get('SMTP_SECURE', port === 465),
    user: env.get('SMTP_USER'),
    password: env.get('SMTP_PASSWORD'),
    from,
  }
}
