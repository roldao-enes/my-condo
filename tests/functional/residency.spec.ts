import { test } from '@japa/runner'
import db from '@adonisjs/lucid/services/db'
import testUtils from '@adonisjs/core/services/test_utils'
import User from '#models/user'
import ResidencyService from '../../app/residency/application/residency_service.js'

test.group('Residency lifecycle', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())

  test('offboarding preserves the person, unit, relationship and audit trail', async ({
    assert,
  }) => {
    const actor = await User.create({
      email: 'admin@example.test',
      password: 'test-password-only',
      fullName: 'Test Admin',
    })
    const service = new ResidencyService()
    const unitId = await service.createUnit(actor.id, { label: '1A', floor: '1' })
    const relationshipId = await service.onboard(actor.id, {
      unitId,
      name: 'Test Resident',
      role: 'owner',
      startDate: '2025-01-01',
    })
    await service.offboard(actor.id, relationshipId, '2025-02-01')
    const relationship = await db.from('occupancies').where('id', relationshipId).first()
    assert.isNotNull(relationship.end_date)
    assert.exists(await db.from('residents').where('id', relationship.resident_id).first())
    assert.exists(await db.from('units').where('id', unitId).first())
    const events = await db.from('audit_events').orderBy('id').select('action')
    assert.deepEqual(
      events.map((event) => event.action),
      ['unit.created', 'resident.onboarded', 'resident.offboarded']
    )
    await assert.rejects(() => service.offboard(actor.id, relationshipId, '2025-02-02'))
    await service.onboard(actor.id, {
      unitId,
      residentId: relationship.resident_id,
      role: 'owner',
      startDate: '2025-03-01',
    })
    assert.lengthOf(await db.from('residents'), 1)
    assert.lengthOf(await db.from('occupancies'), 2)
  })

  test('invalid dates and duplicate active relationships leave no partial writes', async ({
    assert,
  }) => {
    const actor = await User.create({ email: 'admin@example.test', password: 'test-password-only' })
    const service = new ResidencyService()
    const unitId = await service.createUnit(actor.id, { label: '1B', floor: '1' })
    const relationshipId = await service.onboard(actor.id, {
      unitId,
      name: 'Test Resident',
      role: 'tenant',
      startDate: '2025-01-01',
    })
    await assert.rejects(() => service.offboard(actor.id, relationshipId, '2024-12-31'))
    const relationship = await db.from('occupancies').where('id', relationshipId).first()
    assert.isNull(relationship.end_date)
    await assert.rejects(() =>
      service.onboard(actor.id, {
        unitId,
        residentId: relationship.resident_id,
        role: 'tenant',
        startDate: '2025-01-01',
      })
    )
  })
})
