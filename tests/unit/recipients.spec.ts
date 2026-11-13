import { test } from '@japa/runner'
import { buildRecipients } from '../../app/mail/domain/recipients.js'

test.group('Announcement recipients', () => {
  test('skips residents without a valid email', ({ assert }) => {
    const recipients = buildRecipients([
      { residentId: 1, name: 'Ana', email: null, unitLabel: '1A' },
      { residentId: 2, name: 'Rui', email: 'not-an-email', unitLabel: '1B' },
      { residentId: 3, name: 'Eva', email: 'eva@example.test', unitLabel: '2A' },
    ])
    assert.deepEqual(
      recipients.map((recipient) => recipient.email),
      ['eva@example.test']
    )
  })

  test('merges units and duplicate addresses case-insensitively', ({ assert }) => {
    const recipients = buildRecipients([
      { residentId: 1, name: 'Ana', email: 'Ana@Example.test', unitLabel: '1A' },
      { residentId: 1, name: 'Ana', email: 'ana@example.test', unitLabel: '2B' },
      { residentId: 1, name: 'Ana', email: 'ana@example.test', unitLabel: '1A' },
      { residentId: 4, name: 'Ana (spouse)', email: ' ANA@example.test ', unitLabel: '1A' },
    ])
    assert.lengthOf(recipients, 1)
    assert.equal(recipients[0].email, 'ana@example.test')
    assert.deepEqual(recipients[0].unitLabels, ['1A', '2B'])
  })
})
