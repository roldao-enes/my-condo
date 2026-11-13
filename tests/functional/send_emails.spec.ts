import { test } from '@japa/runner'
import ace from '@adonisjs/core/services/ace'
import testUtils from '@adonisjs/core/services/test_utils'
import env from '#start/env'
import SendEmails from '../../commands/send_emails.js'

test.group('send:emails command', (group) => {
  group.each.setup(() => testUtils.db().withGlobalTransaction())
  group.each.setup(() => {
    const previous = { host: env.get('SMTP_HOST'), from: env.get('MAIL_FROM') }
    env.set('SMTP_HOST', '')
    env.set('MAIL_FROM', '')
    return () => {
      env.set('SMTP_HOST', previous.host ?? '')
      env.set('MAIL_FROM', previous.from ?? '')
    }
  })

  test('fails clearly when SMTP is not configured', async ({ assert }) => {
    const command = await ace.create(SendEmails, [])
    await command.exec()
    assert.equal(command.exitCode, 1)
  })

  test('rejects an out-of-range limit', async ({ assert }) => {
    const command = await ace.create(SendEmails, ['--limit=0'])
    await command.exec()
    assert.equal(command.exitCode, 1)
  })
})
