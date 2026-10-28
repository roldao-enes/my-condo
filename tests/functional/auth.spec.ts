import { test } from '@japa/runner'
import testUtils from '@adonisjs/core/services/test_utils'
import db from '@adonisjs/lucid/services/db'
import User from '#models/user'

test.group('Administrator HTTP access', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())

  test('anonymous visitors cannot read residency information', async ({ client }) => {
    for (const path of ['/', '/units', '/residents']) {
      const response = await client.get(path).header('Accept', 'text/html').redirects(0)
      response.assertStatus(302)
      response.assertHeader('location', '/login')
    }
  })

  test('writes without a CSRF token are rejected', async ({ client, assert }) => {
    const user = await User.create({ email: 'admin@example.test', password: 'test-password-only' })
    const response = await client
      .post('/units')
      .header('Accept', 'application/json')
      .loginAs(user)
      .form({ label: '1A', floor: '1' })
      .redirects(0)
    response.assertStatus(302)
    assert.isNull(await db.from('units').where('label', '1A').first())
  })

  test('authenticated writes persist and protected pages expose no password', async ({
    client,
    assert,
  }) => {
    const user = await User.create({
      email: 'admin@example.test',
      password: 'test-password-only',
      fullName: 'Test Admin',
    })
    const response = await client
      .post('/units')
      .loginAs(user)
      .withCsrfToken()
      .form({ label: '1A', floor: '1' })
      .redirects(0)
    response.assertStatus(302)
    assert.exists(await db.from('units').where('label', '1A').first())
    const page = await client.get('/units').loginAs(user)
    page.assertStatus(200)
    assert.include(page.text(), 'residency/index')
    assert.include(page.text(), user.email)
    assert.notInclude(page.text(), user.password)
    assert.notInclude(page.text(), 'test-password-only')
  })

  test('credential verification accepts valid credentials and rejects invalid ones', async ({
    client,
  }) => {
    await User.create({ email: 'admin@example.test', password: 'test-password-only' })
    const invalid = await client
      .post('/login')
      .withCsrfToken()
      .form({ email: 'admin@example.test', password: 'incorrect' })
      .redirects(0)
    invalid.assertStatus(302)
    invalid.assertHeader('location', '/login')
    invalid.assertFlashMessage('errors', { email: 'Email ou palavra-passe incorretos.' })
    const valid = await client
      .post('/login')
      .withCsrfToken()
      .form({ email: 'admin@example.test', password: 'test-password-only' })
      .redirects(0)
    valid.assertStatus(302)
    valid.assertHeader('location', '/')
  })
})
