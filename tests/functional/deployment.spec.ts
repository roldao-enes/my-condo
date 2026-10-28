import { test } from '@japa/runner'
import { mock } from 'node:test'
import db from '@adonisjs/lucid/services/db'

test.group('Container health probes', () => {
  test('readiness verifies PostgreSQL and preserves the original health endpoint', async ({
    client,
  }) => {
    for (const path of ['/health', '/health/ready', '/health/live']) {
      const response = await client.get(path)
      response.assertStatus(200)
      response.assertBody({ status: 'ok' })
      response.assertHeader('cache-control', 'no-store')
    }
  })

  test('database failure affects readiness but not process liveness', async ({ client }) => {
    const query = mock.method(db, 'rawQuery', () => {
      throw new Error('test database unavailable')
    })
    try {
      const live = await client.get('/health/live')
      live.assertStatus(200)
      for (const path of ['/health', '/health/ready']) {
        const ready = await client.get(path)
        ready.assertStatus(503)
        ready.assertBody({ status: 'unavailable' })
      }
    } finally {
      query.mock.restore()
    }
  })
})
