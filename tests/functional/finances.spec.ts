import { test } from '@japa/runner'
import testUtils from '@adonisjs/core/services/test_utils'
import db from '@adonisjs/lucid/services/db'
import User from '#models/user'
import FinanceService from '../../app/finances/finance_service.js'
import ResidencyService from '../../app/residency/application/residency_service.js'

test.group('Condominium finances', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())

  test('imports atomically, skips overlapping rows and preserves classifications', async ({
    assert,
  }) => {
    const user = await User.create({
      email: 'finance@example.test',
      password: 'test-password-only',
    })
    const service = new FinanceService()
    const first = '2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11'
    const second = '2025-01-04\t2025-01-04\tPayment\t100\t5022,11'
    assert.deepEqual(await service.importStatement(user.id, first), { imported: 1, skipped: 0 })
    const transaction = await db.from('finance_transactions').first()
    await service.classify(user.id, transaction.id, 'recurring')
    assert.deepEqual(await service.importStatement(user.id, `${first}\n${second}`), {
      imported: 1,
      skipped: 1,
    })
    const classified = await db.from('finance_transactions').where('id', transaction.id).first()
    assert.equal(classified.category, 'recurring')
    await assert.rejects(() =>
      service.importStatement(user.id, `${second}\n2025-02-30\t2025-01-01\tInvalid\t1\t1`)
    )
    assert.lengthOf(await db.from('finance_transactions'), 2)
  })

  test('matches partial payments without overpaying a charge or spending income twice', async ({
    assert,
  }) => {
    const user = await User.create({
      email: 'finance@example.test',
      password: 'test-password-only',
    })
    const residency = new ResidencyService()
    const unitId = await residency.createUnit(user.id, { label: '1A', floor: '1' })
    await residency.onboard(user.id, {
      unitId,
      name: 'Resident',
      role: 'owner',
      startDate: '2025-01-01',
    })
    const resident = await db.from('residents').first()
    const service = new FinanceService()
    const chargeId = await service.createCharge(user.id, {
      residentId: resident.id,
      unitId,
      description: 'January dues',
      dueDate: '2025-01-31',
      amount: 10000,
    })
    const otherChargeId = await service.createCharge(user.id, {
      residentId: resident.id,
      unitId,
      description: 'February dues',
      dueDate: '2025-02-28',
      amount: 10000,
    })
    await service.importStatement(user.id, '2025-01-03\t2025-01-03\tPayment\t120\t5000')
    const transaction = await db.from('finance_transactions').first()
    await service.allocate(user.id, transaction.id, chargeId, 6000)
    await service.allocate(user.id, transaction.id, chargeId, 4000)
    await assert.rejects(() => service.allocate(user.id, transaction.id, chargeId, 1))
    await assert.rejects(() => service.allocate(user.id, transaction.id, otherChargeId, 2001))
    await service.allocate(user.id, transaction.id, otherChargeId, 2000)
    const data = await service.list()
    assert.equal(
      data.payments.reduce((sum, payment) => sum + payment.amount, 0),
      12000
    )
    assert.lengthOf(data.accounts, 1)
    await service.unallocate(user.id, data.payments[0].id)
    assert.lengthOf(await db.from('finance_payments'), 1)
    assert.exists(
      await db.from('audit_events').where('action', 'finance.payment_unmatched').first()
    )
  })

  test('normalizes newest-first statements so the latest same-day balance is correct', async ({
    assert,
  }) => {
    const user = await User.create({
      email: 'finance@example.test',
      password: 'test-password-only',
    })
    const service = new FinanceService()
    const newest = '2025-01-03\t2025-01-03\tStamp duty\t-0,30\t4921,81'
    const oldest = '2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11'
    await service.importStatement(user.id, `${newest}\n${oldest}`)
    const data = await service.list()
    assert.equal(data.transactions[0].balance, 492181)
    assert.equal(data.transactions[0].description, 'Stamp duty')
    assert.deepEqual(await service.importStatement(user.id, `${oldest}\n${newest}`), {
      imported: 0,
      skipped: 2,
    })
  })

  test('protects finance pages and mutations with authentication and CSRF', async ({
    client,
    assert,
  }) => {
    const response = await client.get('/finances').redirects(0)
    response.assertStatus(302)
    response.assertHeader('location', '/login')
    const user = await User.create({
      email: 'finance@example.test',
      password: 'test-password-only',
    })
    const denied = await client.post('/finances/charges').loginAs(user).form({}).redirects(0)
    denied.assertStatus(302)
    assert.lengthOf(await db.from('finance_charges'), 0)
  })

  test('uploads CSV through HTTP and rejects a malformed statement without partial writes', async ({
    client,
    assert,
  }) => {
    const user = await User.create({
      email: 'finance@example.test',
      password: 'test-password-only',
    })
    const valid = '2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11'
    const upload = await client
      .post('/finances/import')
      .loginAs(user)
      .withCsrfToken()
      .file('statement', Buffer.from(valid), { filename: 'statement.csv', contentType: 'text/csv' })
      .redirects(0)
    upload.assertStatus(302)
    upload.assertFlashMessage('success', '1 movimentos importados; 0 duplicados ignorados.')
    const malformed = await client
      .post('/finances/import')
      .loginAs(user)
      .withCsrfToken()
      .file('statement', Buffer.from(`${valid}\n2025-02-30\t2025-01-01\tInvalid\t1\t1`), {
        filename: 'statement.csv',
        contentType: 'text/csv',
      })
      .redirects(0)
    malformed.assertStatus(302)
    malformed.assertFlashMessage('error', 'Movimento 2: Data invalida.')
    assert.lengthOf(await db.from('finance_transactions'), 1)
    const page = await client.get('/finances').loginAs(user)
    page.assertStatus(200)
    assert.include(page.text(), 'finances/index')
    assert.include(page.text(), 'Bank fee')
  })
})
