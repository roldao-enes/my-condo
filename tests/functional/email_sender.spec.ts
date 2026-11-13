import { test } from '@japa/runner'
import db from '@adonisjs/lucid/services/db'
import testUtils from '@adonisjs/core/services/test_utils'
import User from '#models/user'
import EmailSender, {
  type MailTransport,
  type OutgoingEmail,
} from '../../app/mail/application/email_sender.js'
import AnnouncementService from '../../app/mail/application/announcement_service.js'
import ResidencyService from '../../app/residency/application/residency_service.js'
import { MAX_ATTEMPTS } from '../../app/mail/domain/retry.js'

class FakeTransport implements MailTransport {
  sent: OutgoingEmail[] = []
  failing = false
  async send(message: OutgoingEmail) {
    if (this.failing) throw new Error('connect ECONNREFUSED')
    this.sent.push(message)
  }
}

async function queueOne() {
  const actor = await User.create({
    email: 'admin@example.test',
    password: 'test-password-only',
    fullName: 'Test Admin',
  })
  const residency = new ResidencyService()
  const unitId = await residency.createUnit(actor.id, { label: '1A', floor: '1' })
  await residency.onboard(actor.id, {
    unitId,
    name: 'Ana',
    email: 'ana@example.test',
    role: 'owner',
    startDate: '2025-01-01',
  })
  const announcements = new AnnouncementService()
  const id = await announcements.createDraft(actor.id, { subject: 'Hi', body: 'Body text' })
  await announcements.queue(actor.id, id)
  const job = await db.from('email_jobs').where('announcement_id', id).firstOrFail()
  return job.id as number
}

test.group('Email sender', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())

  test('sends a due job once and records the attempt', async ({ assert }) => {
    const jobId = await queueOne()
    const transport = new FakeTransport()
    const sender = new EmailSender(transport, () => new Date(Date.now() + 1000))
    assert.equal(await sender.countDue(), 1)
    assert.deepEqual(await sender.run(10), { sent: 1, retrying: 0, failed: 0 })
    assert.deepEqual(await sender.run(10), { sent: 0, retrying: 0, failed: 0 })

    assert.lengthOf(transport.sent, 1)
    assert.deepEqual(transport.sent[0].to, { name: 'Ana', address: 'ana@example.test' })
    assert.equal(transport.sent[0].subject, 'Hi')
    assert.equal(transport.sent[0].text, 'Body text')
    const job = await db.from('email_jobs').where('id', jobId).firstOrFail()
    assert.equal(job.status, 'sent')
    assert.equal(job.attempts, 1)
    assert.isNotNull(job.sent_at)
    const attempts = await db.from('email_attempts').where('email_job_id', jobId)
    assert.deepEqual(
      attempts.map((attempt) => attempt.outcome),
      ['ok']
    )
  })

  test('schedules a retry after a failure and skips the job until it is due', async ({
    assert,
  }) => {
    const jobId = await queueOne()
    const transport = new FakeTransport()
    transport.failing = true
    const start = Date.now() + 1000
    let clock = start
    const sender = new EmailSender(transport, () => new Date(clock))

    assert.deepEqual(await sender.run(10), { sent: 0, retrying: 1, failed: 0 })
    let job = await db.from('email_jobs').where('id', jobId).firstOrFail()
    assert.equal(job.status, 'pending')
    assert.equal(job.attempts, 1)
    assert.include(job.last_error, 'ECONNREFUSED')
    assert.equal(new Date(job.next_attempt_at).getTime(), start + 5 * 60_000)

    assert.deepEqual(await sender.run(10), { sent: 0, retrying: 0, failed: 0 })
    assert.equal(await sender.countDue(), 0)

    transport.failing = false
    clock = start + 5 * 60_000
    assert.deepEqual(await sender.run(10), { sent: 1, retrying: 0, failed: 0 })
    job = await db.from('email_jobs').where('id', jobId).firstOrFail()
    assert.equal(job.status, 'sent')
    assert.equal(job.attempts, 2)
    assert.isNull(job.last_error)
    const attempts = await db.from('email_attempts').where('email_job_id', jobId).orderBy('id')
    assert.deepEqual(
      attempts.map((attempt) => attempt.outcome),
      ['error', 'ok']
    )
  })

  test('gives up after the maximum number of attempts', async ({ assert }) => {
    const jobId = await queueOne()
    const transport = new FakeTransport()
    transport.failing = true
    let clock = Date.now() + 1000
    const sender = new EmailSender(transport, () => new Date(clock))

    let last = { sent: 0, retrying: 0, failed: 0 }
    for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
      last = await sender.run(10)
      clock += 24 * 60 * 60_000
    }
    assert.deepEqual(last, { sent: 0, retrying: 0, failed: 1 })
    const job = await db.from('email_jobs').where('id', jobId).firstOrFail()
    assert.equal(job.status, 'failed')
    assert.equal(job.attempts, MAX_ATTEMPTS)
    assert.lengthOf(await db.from('email_attempts').where('email_job_id', jobId), MAX_ATTEMPTS)
    assert.deepEqual(await sender.run(10), { sent: 0, retrying: 0, failed: 0 })
  })

  test('reclaims a job whose earlier run died after the lease expired', async ({ assert }) => {
    const jobId = await queueOne()
    const start = Date.now() + 1000
    // Claim without finishing: mark the job as leased by a run that never completed.
    await db
      .from('email_jobs')
      .where('id', jobId)
      .update({ status: 'sending', next_attempt_at: new Date(start + 10 * 60_000) })

    const transport = new FakeTransport()
    assert.deepEqual(await new EmailSender(transport, () => new Date(start)).run(10), {
      sent: 0,
      retrying: 0,
      failed: 0,
    })
    assert.deepEqual(
      await new EmailSender(transport, () => new Date(start + 11 * 60_000)).run(10),
      { sent: 1, retrying: 0, failed: 0 }
    )
    assert.lengthOf(transport.sent, 1)
  })

  test('honours the per-run limit', async ({ assert }) => {
    const actor = await User.create({
      email: 'admin@example.test',
      password: 'test-password-only',
      fullName: 'Test Admin',
    })
    const residency = new ResidencyService()
    const unitId = await residency.createUnit(actor.id, { label: '1A', floor: '1' })
    for (const email of ['a@example.test', 'b@example.test', 'c@example.test']) {
      await residency.onboard(actor.id, {
        unitId,
        name: email,
        email,
        role: 'occupant',
        startDate: '2025-01-01',
      })
    }
    const announcements = new AnnouncementService()
    const id = await announcements.createDraft(actor.id, { subject: 'Hi', body: 'Body' })
    await announcements.queue(actor.id, id)
    const transport = new FakeTransport()
    const sender = new EmailSender(transport, () => new Date(Date.now() + 1000))
    assert.deepEqual(await sender.run(2), { sent: 2, retrying: 0, failed: 0 })
    assert.equal(await sender.countDue(), 1)
  })
})
