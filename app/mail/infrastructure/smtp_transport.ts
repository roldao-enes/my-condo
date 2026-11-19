import nodemailer, { type Transporter } from 'nodemailer'
import type { MailTransport, OutgoingEmail } from '../application/email_sender.js'

export interface SmtpSettings {
  host: string
  port: number
  secure: boolean
  user?: string
  password?: string
  from: string
}

export default class SmtpTransport implements MailTransport {
  readonly #transporter: Transporter
  readonly #from: string

  constructor(settings: SmtpSettings) {
    this.#from = settings.from
    this.#transporter = nodemailer.createTransport({
      host: settings.host,
      port: settings.port,
      secure: settings.secure,
      auth: settings.user ? { user: settings.user, pass: settings.password } : undefined,
      connectionTimeout: 15_000,
      greetingTimeout: 15_000,
      socketTimeout: 30_000,
    })
  }

  async send(message: OutgoingEmail) {
    await this.#transporter.sendMail({
      from: this.#from,
      to: { name: message.to.name, address: message.to.address },
      subject: message.subject,
      text: message.text,
    })
  }

  close() {
    this.#transporter.close()
  }
}
