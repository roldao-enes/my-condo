import db from '@adonisjs/lucid/services/db'
import { CLAIM_LEASE_MINUTES, retryDelayMinutes } from '../domain/retry.js'

export interface OutgoingEmail {
  to: { name: string; address: string }
  subject: string
  text: string
}

export interface MailTransport {
  send(message: OutgoingEmail): Promise<void>
}

export interface SendSummary {
  sent: number
  retrying: number
  failed: number
}

interface ClaimedJob {
  id: number
  attempts: number
  recipientName: string
  recipientEmail: string
  subject: string
  body: string
}

const addMinutes = (date: Date, minutes: number) => new Date(date.getTime() + minutes * 60_000)

function describeError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error)
  return message.slice(0, 500)
}

/**
 * Sends due email jobs once and returns. Delivery is at-least-once: a job claimed by a
 * run that dies mid-send becomes claimable again after the lease expires.
 */
export default class EmailSender {
  constructor(
    private readonly transport: MailTransport,
    private readonly now: () => Date = () => new Date()
  ) {}

  async countDue() {
    const row = await db
      .from('email_jobs')
      .whereIn('status', ['pending', 'sending'])
      .where('next_attempt_at', '<=', this.now())
      .count('* as total')
      .first()
    return Number(row?.total ?? 0)
  }

  async run(limit: number): Promise<SendSummary> {
    const summary: SendSummary = { sent: 0, retrying: 0, failed: 0 }
    for (let processed = 0; processed < limit; processed++) {
      const job = await this.#claim()
      if (!job) break
      const outcome = await this.#deliver(job)
      summary[outcome]++
    }
    return summary
  }

  async #claim(): Promise<ClaimedJob | null> {
    const now = this.now()
    return db.transaction(async (trx) => {
      const job = await trx
        .from('email_jobs')
        .join('announcements', 'announcements.id', 'email_jobs.announcement_id')
        .whereIn('email_jobs.status', ['pending', 'sending'])
        .where('email_jobs.next_attempt_at', '<=', now)
        .orderBy('email_jobs.next_attempt_at')
        .orderBy('email_jobs.id')
        .select(
          'email_jobs.id as id',
          'email_jobs.attempts as attempts',
          'email_jobs.recipient_name as recipientName',
          'email_jobs.recipient_email as recipientEmail',
          'announcements.subject as subject',
          'announcements.body as body'
        )
        .forUpdate()
        .skipLocked()
        .first()
      if (!job) return null
      await trx
        .from('email_jobs')
        .where('id', job.id)
        .update({
          status: 'sending',
          next_attempt_at: addMinutes(now, CLAIM_LEASE_MINUTES),
        })
      return job as ClaimedJob
    })
  }

  async #deliver(job: ClaimedJob): Promise<keyof SendSummary> {
    const attempts = job.attempts + 1
    try {
      await this.transport.send({
        to: { name: job.recipientName, address: job.recipientEmail },
        subject: job.subject,
        text: job.body,
      })
    } catch (error) {
      const message = describeError(error)
      const delay = retryDelayMinutes(attempts)
      await db.transaction(async (trx) => {
        await trx
          .table('email_attempts')
          .insert({ email_job_id: job.id, outcome: 'error', error: message })
        await trx
          .from('email_jobs')
          .where('id', job.id)
          .update({
            status: delay === null ? 'failed' : 'pending',
            attempts,
            last_error: message,
            next_attempt_at: addMinutes(this.now(), delay ?? 0),
          })
      })
      return delay === null ? 'failed' : 'retrying'
    }
    await db.transaction(async (trx) => {
      await trx.table('email_attempts').insert({ email_job_id: job.id, outcome: 'ok' })
      await trx
        .from('email_jobs')
        .where('id', job.id)
        .update({ status: 'sent', attempts, last_error: null, sent_at: this.now() })
    })
    return 'sent'
  }
}
