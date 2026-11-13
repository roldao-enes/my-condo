import { test } from '@japa/runner'
import { MAX_ATTEMPTS, retryDelayMinutes } from '../../app/mail/domain/retry.js'

test.group('Email retry schedule', () => {
  test('backs off after each failure and gives up after the last attempt', ({ assert }) => {
    assert.deepEqual(
      [1, 2, 3, 4].map((attempts) => retryDelayMinutes(attempts)),
      [5, 30, 180, 720]
    )
    assert.equal(MAX_ATTEMPTS, 5)
    assert.isNull(retryDelayMinutes(MAX_ATTEMPTS))
    assert.isNull(retryDelayMinutes(MAX_ATTEMPTS + 3))
  })

  test('rejects nonsensical attempt counts', ({ assert }) => {
    assert.throws(() => retryDelayMinutes(0))
    assert.throws(() => retryDelayMinutes(1.5))
  })
})
