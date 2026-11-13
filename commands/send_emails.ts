import { BaseCommand, flags } from '@adonisjs/core/ace'
import type { CommandOptions } from '@adonisjs/core/types/ace'
import { smtpSettings } from '#config/mail'
import EmailSender from '../app/mail/application/email_sender.js'
import SmtpTransport from '../app/mail/infrastructure/smtp_transport.js'
import { describeFailure, recordAudit } from '../app/audit/audit.js'
import type { AuditActor } from '../app/audit/rotating_audit_log.js'

const EVENT = 'email.send'
const ACTOR: AuditActor = { type: 'console' }

export default class SendEmails extends BaseCommand {
  static commandName = 'send:emails'
  static description = 'Send queued announcement emails once and exit (safe to schedule)'
  static options: CommandOptions = { startApp: true }

  @flags.number({ description: 'Maximum number of emails to process in this run', default: 50 })
  declare limit: number

  @flags.boolean({ description: 'Only report how many emails are due; send nothing' })
  declare dryRun: boolean

  async run() {
    const limit = Number(this.limit)
    if (!Number.isInteger(limit) || limit < 1 || limit > 1000) {
      this.logger.error('--limit must be an integer between 1 and 1000.')
      this.exitCode = 1
      return
    }

    const settings = smtpSettings()
    if (!settings) {
      await recordAudit({
        event: EVENT,
        outcome: 'failure',
        message: 'Console operator failed to send emails: SMTP_HOST and MAIL_FROM are not set',
        actor: ACTOR,
        failure: describeFailure('configuration: SMTP_HOST and MAIL_FROM are required'),
        context: {},
      })
      this.logger.error('SMTP_HOST and MAIL_FROM must be set to send emails.')
      this.exitCode = 1
      return
    }

    const transport = new SmtpTransport(settings)
    const sender = new EmailSender(transport)
    try {
      if (this.dryRun) {
        this.logger.info(`${await sender.countDue()} email(s) are due. Nothing was sent.`)
        return
      }
      const summary = await sender.run(limit)
      await recordAudit({
        event: EVENT,
        outcome: summary.failed > 0 ? 'failure' : 'success',
        message: `Console operator ran send:emails: ${summary.sent} sent, ${summary.retrying} to retry, ${summary.failed} failed`,
        actor: ACTOR,
        ...(summary.failed > 0
          ? { failure: describeFailure('some emails exhausted their retries') }
          : {}),
        context: { ...summary, limit },
      })
      this.logger.info(
        `Sent ${summary.sent}; ${summary.retrying} will be retried; ${summary.failed} failed permanently.`
      )
      if (summary.failed > 0) this.exitCode = 1
    } catch (error) {
      await recordAudit({
        event: EVENT,
        outcome: 'failure',
        message: 'Console operator failed to run send:emails',
        actor: ACTOR,
        failure: describeFailure(error, true),
        context: { limit },
      })
      throw error
    } finally {
      transport.close()
    }
  }
}
