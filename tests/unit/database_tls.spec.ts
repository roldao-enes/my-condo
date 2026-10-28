import { test } from '@japa/runner'
import { databaseTls } from '../../app/deployment/database_tls.js'

test.group('Deployment database TLS', () => {
  test('allows explicit plaintext for a local private database', ({ assert }) => {
    assert.isFalse(databaseTls('disable'))
  })

  test('always verifies managed database certificates', ({ assert }) => {
    assert.deepEqual(databaseTls('verify-full'), { rejectUnauthorized: true })
  })

  test('accepts a secret-provided CA with literal or escaped newlines', ({ assert }) => {
    const certificate = '-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----'
    assert.deepEqual(databaseTls('verify-full', certificate.replace(/\n/g, '\\n')), {
      rejectUnauthorized: true,
      ca: certificate,
    })
    assert.deepEqual(databaseTls('verify-full', certificate), {
      rejectUnauthorized: true,
      ca: certificate,
    })
  })

  test('does not silently ignore a supplied CA when TLS is disabled', ({ assert }) => {
    assert.throws(() => databaseTls('disable', 'certificate'), /DB_SSL_MODE=verify-full/)
  })
})
