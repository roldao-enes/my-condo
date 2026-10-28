import { test } from '@japa/runner'
import { readFile, rm } from 'node:fs/promises'
import ace from '@adonisjs/core/services/ace'
import testUtils from '@adonisjs/core/services/test_utils'
import User from '#models/user'
import CreateAdmin from '../../commands/create_admin.js'
import { auditLog } from '../../app/audit/audit.js'
import type { AuditEntry } from '../../app/audit/rotating_audit_log.js'

async function entries(): Promise<AuditEntry[]> {
  const content = await readFile(auditLog().activePath, 'utf8').catch(() => '')
  return content
    .split('\n')
    .filter(Boolean)
    .map((line) => JSON.parse(line))
}

test.group('Audit logging of state-changing commands', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())
  group.each.setup(() => rm(auditLog().directory, { recursive: true, force: true }))

  test('records login failures, login and logout with timestamps', async ({ client, assert }) => {
    const user = await User.create({ email: 'admin@example.test', password: 'test-password-only' })
    await client
      .post('/login')
      .withCsrfToken()
      .form({ email: 'admin@example.test', password: 'incorrect' })
      .redirects(0)
    await client.post('/login').withCsrfToken().form({ email: 'not-an-email' }).redirects(0)
    await client
      .post('/login')
      .withCsrfToken()
      .form({ email: 'admin@example.test', password: 'test-password-only' })
      .redirects(0)
    await client.post('/logout').loginAs(user).withCsrfToken().redirects(0)

    const log = await entries()
    for (const entry of log) assert.isFalse(Number.isNaN(Date.parse(entry.timestamp)))
    assert.deepEqual(
      log.map(({ event, outcome, message }) => ({ event, outcome, message })),
      [
        {
          event: 'auth.login',
          outcome: 'failure',
          message:
            'Anonymous visitor failed to log in as "admin@example.test": invalid email or password',
        },
        {
          event: 'auth.login',
          outcome: 'failure',
          message: 'Anonymous visitor failed to log in: invalid input in field(s) email, password',
        },
        {
          event: 'auth.login',
          outcome: 'success',
          message: `User #${user.id} (admin@example.test) logged in`,
        },
        {
          event: 'auth.logout',
          outcome: 'success',
          message: `User #${user.id} (admin@example.test) logged out`,
        },
      ]
    )
    assert.equal(log[0].failure?.kind, 'invalid_credentials')
    assert.notInclude(JSON.stringify(log), 'incorrect')
    assert.notInclude(JSON.stringify(log), 'test-password-only')
  })

  test('records residency changes and their failure modes', async ({ client, assert }) => {
    const user = await User.create({ email: 'admin@example.test', password: 'test-password-only' })
    const actor = `User #${user.id} (admin@example.test)`
    await client
      .post('/units')
      .loginAs(user)
      .withCsrfToken()
      .form({ label: '1A', floor: '1' })
      .redirects(0)
    await client.post('/units').loginAs(user).withCsrfToken().form({ floor: '1' }).redirects(0)
    await client
      .post('/occupancies/999999/offboard')
      .loginAs(user)
      .withCsrfToken()
      .form({ endDate: '2025-01-01' })
      .redirects(0)

    const log = await entries()
    assert.lengthOf(log, 3)
    assert.match(
      log[0].message,
      /^User #\d+ \(admin@example\.test\) created unit #\d+ "1A" on floor "1"$/
    )
    assert.equal(
      log[1].message,
      `${actor} failed to create a unit: invalid input in field(s) label`
    )
    assert.equal(
      log[2].message,
      `${actor} failed to offboard occupancy #999999 on 2025-01-01: not found: Relacao nao encontrada.`
    )
    assert.deepEqual(
      log.map((entry) => [entry.event, entry.outcome, entry.failure?.kind]),
      [
        ['residency.unit.create', 'success', undefined],
        ['residency.unit.create', 'failure', 'invalid_input'],
        ['residency.resident.offboard', 'failure', 'not_found'],
      ]
    )
    assert.equal(log[0].context?.method, 'POST')
    assert.equal(log[0].context?.url, '/units')
  })

  test('records finance changes and their failure modes', async ({ client, assert }) => {
    const user = await User.create({ email: 'admin@example.test', password: 'test-password-only' })
    const actor = `User #${user.id} (admin@example.test)`
    await client
      .post('/finances/tags')
      .loginAs(user)
      .withCsrfToken()
      .form({ name: 'Water', pattern: 'AGUAS', matchType: 'contains', category: 'recurring' })
      .redirects(0)
    await client.post('/finances/tags/abc/remove').loginAs(user).withCsrfToken().redirects(0)
    await client.post('/finances/tags/999999/remove').loginAs(user).withCsrfToken().redirects(0)
    await client
      .post('/finances/charges')
      .loginAs(user)
      .withCsrfToken()
      .form({
        residentId: 1,
        unitId: 1,
        description: 'Quota',
        dueDate: '2025-01-01',
        amount: 'abc',
      })
      .redirects(0)

    const log = await entries()
    assert.lengthOf(log, 4)
    assert.match(
      log[0].message,
      /^User #\d+ \(admin@example\.test\) created tag #\d+ "Water" \(contains match on "AGUAS", category recurring\)$/
    )
    assert.equal(
      log[1].message,
      `${actor} failed to remove tag "abc": invalid input: Identificador invalido.`
    )
    assert.equal(
      log[2].message,
      `${actor} failed to remove tag #999999: not found: Etiqueta nao encontrada.`
    )
    assert.equal(
      log[3].message,
      `${actor} failed to register a charge: invalid input: Montante invalido.`
    )
    assert.deepEqual(
      log.map((entry) => [entry.event, entry.outcome, entry.failure?.kind]),
      [
        ['finance.tag.create', 'success', undefined],
        ['finance.tag.delete', 'failure', 'invalid_input'],
        ['finance.tag.delete', 'failure', 'not_found'],
        ['finance.charge.create', 'failure', 'invalid_input'],
      ]
    )
  })

  test('records administrator provisioning from the console', async ({ assert }) => {
    const previous = { ...process.env }
    const run = async () => {
      const command = await ace.create(CreateAdmin, ['--from-env'])
      await command.exec()
    }
    try {
      delete process.env.ADMIN_EMAIL
      delete process.env.ADMIN_PASSWORD
      await run()
      process.env.ADMIN_EMAIL = 'cloud-admin@example.test'
      process.env.ADMIN_PASSWORD = 'test-secret-password-only'
      await run()
      await run()
    } finally {
      process.env = previous
    }
    const user = await User.findByOrFail('email', 'cloud-admin@example.test')
    const log = await entries()
    assert.deepEqual(
      log.map(({ event, outcome, actor, failure }) => [event, outcome, actor.type, failure?.kind]),
      [
        ['admin.create', 'failure', 'console', 'invalid_input'],
        ['admin.create', 'success', 'console', undefined],
        ['admin.create', 'failure', 'console', 'conflict'],
      ]
    )
    assert.equal(
      log[1].message,
      `Console operator created administrator #${user.id} (cloud-admin@example.test) via create:admin --from-env`
    )
    assert.equal(
      log[2].message,
      'Console operator failed to create administrator "cloud-admin@example.test": conflict: an account with this email already exists; no account was changed'
    )
    assert.notInclude(JSON.stringify(log), 'test-secret-password-only')
  })
})
