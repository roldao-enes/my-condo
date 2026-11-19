import { test } from '@japa/runner'
import db from '@adonisjs/lucid/services/db'
import testUtils from '@adonisjs/core/services/test_utils'
import User from '#models/user'
import AnnouncementService from '../../app/mail/application/announcement_service.js'
import ResidencyService from '../../app/residency/application/residency_service.js'

async function setup() {
  const actor = await User.create({
    email: 'admin@example.test',
    password: 'test-password-only',
    fullName: 'Test Admin',
  })
  const residency = new ResidencyService()
  const unitA = await residency.createUnit(actor.id, { label: '1A', floor: '1' })
  const unitB = await residency.createUnit(actor.id, { label: '1B', floor: '1' })
  return { actor, residency, unitA, unitB }
}

test.group('Announcement queueing', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())

  test('snapshots only active residents that have an email', async ({ assert }) => {
    const { actor, residency, unitA, unitB } = await setup()
    await residency.onboard(actor.id, {
      unitId: unitA,
      name: 'Ana',
      email: 'ana@example.test',
      role: 'owner',
      startDate: '2025-01-01',
    })
    await residency.onboard(actor.id, {
      unitId: unitB,
      name: 'No Email',
      role: 'owner',
      startDate: '2025-01-01',
    })
    const leaverId = await residency.onboard(actor.id, {
      unitId: unitB,
      name: 'Leaver',
      email: 'leaver@example.test',
      role: 'tenant',
      startDate: '2025-01-01',
    })
    await residency.offboard(actor.id, leaverId, '2025-02-01')

    const service = new AnnouncementService()
    const id = await service.createDraft(actor.id, { subject: ' Hello ', body: ' World ' })
    assert.equal(await service.queue(actor.id, id), 1)

    const jobs = await db.from('email_jobs').where('announcement_id', id)
    assert.lengthOf(jobs, 1)
    assert.equal(jobs[0].recipient_email, 'ana@example.test')
    assert.equal(jobs[0].unit_labels, '1A')
    assert.equal(jobs[0].status, 'pending')
    const announcement = await db.from('announcements').where('id', id).first()
    assert.equal(announcement.status, 'queued')
    assert.equal(announcement.subject, 'Hello')
    const actions = await db.from('audit_events').orderBy('id').select('action')
    assert.include(
      actions.map((event) => event.action),
      'announcement.queued'
    )
  })

  test('can be limited to one unit and cannot be queued twice', async ({ assert }) => {
    const { actor, residency, unitA, unitB } = await setup()
    for (const [unitId, email] of [
      [unitA, 'a@example.test'],
      [unitB, 'b@example.test'],
    ] as const) {
      await residency.onboard(actor.id, {
        unitId,
        name: email,
        email,
        role: 'owner',
        startDate: '2025-01-01',
      })
    }
    const service = new AnnouncementService()
    const id = await service.createDraft(actor.id, { subject: 'Unit only', body: 'Body' })
    assert.equal(await service.queue(actor.id, id, { unitId: unitB }), 1)
    const [job] = await db.from('email_jobs').where('announcement_id', id)
    assert.equal(job.recipient_email, 'b@example.test')
    await assert.rejects(() => service.queue(actor.id, id), /ja foi colocado em fila/)
    assert.lengthOf(await db.from('email_jobs').where('announcement_id', id), 1)
  })

  test('refuses to queue when nobody can be reached', async ({ assert }) => {
    const { actor } = await setup()
    const service = new AnnouncementService()
    const id = await service.createDraft(actor.id, { subject: 'Nobody', body: 'Body' })
    await assert.rejects(() => service.queue(actor.id, id), /destinatarios/)
    const announcement = await db.from('announcements').where('id', id).first()
    assert.equal(announcement.status, 'draft')
  })
})
