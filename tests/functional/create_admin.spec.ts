import { test } from '@japa/runner'
import ace from '@adonisjs/core/services/ace'
import hash from '@adonisjs/core/services/hash'
import testUtils from '@adonisjs/core/services/test_utils'
import User from '#models/user'
import CreateAdmin from '../../commands/create_admin.js'

test.group('Cloud administrator provisioning', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())
  group.each.setup(() => {
    const previous = {
      ADMIN_NAME: process.env.ADMIN_NAME,
      ADMIN_EMAIL: process.env.ADMIN_EMAIL,
      ADMIN_PASSWORD: process.env.ADMIN_PASSWORD,
    }
    for (const key of Object.keys(previous)) delete process.env[key]
    return () => {
      for (const [key, value] of Object.entries(previous)) {
        if (value === undefined) delete process.env[key]
        else process.env[key] = value
      }
    }
  })

  test('missing secret values fail without prompting or creating an account', async ({
    assert,
  }) => {
    const command = await ace.create(CreateAdmin, ['--from-env'])
    await command.exec()
    assert.equal(command.exitCode, 1)
    assert.isNull(await User.query().first())
  })

  test('creates a hashed credential and refuses to replace an existing account', async ({
    assert,
  }) => {
    process.env.ADMIN_EMAIL = ' CLOUD-ADMIN@example.test '
    process.env.ADMIN_PASSWORD = 'test-secret-password-only'
    const command = await ace.create(CreateAdmin, ['--from-env'])
    await command.exec()
    assert.notEqual(command.exitCode, 1)
    const user = await User.findByOrFail('email', 'cloud-admin@example.test')
    assert.isTrue(await hash.verify(user.password, process.env.ADMIN_PASSWORD))
    const passwordHash = user.password
    process.env.ADMIN_PASSWORD = 'different-test-password'
    const duplicate = await ace.create(CreateAdmin, ['--from-env'])
    await duplicate.exec()
    assert.equal(duplicate.exitCode, 1)
    await user.refresh()
    assert.equal(user.password, passwordHash)
  })
})
