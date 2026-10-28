import { test } from '@japa/runner'
import { validateOccupancyDates } from '../../app/residency/domain/occupancy.js'

test.group('Occupancy dates', () => {
  test('accepts an ongoing relationship and a same-day departure', () => {
    validateOccupancyDates('2026-10-27')
    validateOccupancyDates('2026-10-27', '2026-10-27')
  })

  test('rejects departure before arrival', ({ assert }) => {
    assert.throws(() => validateOccupancyDates('2026-10-27', '2026-10-26'))
  })

  test('rejects malformed and impossible calendar dates', ({ assert }) => {
    for (const date of ['2026-02-30', '2025-02-29', '27/10/2026', 'invalid']) {
      assert.throws(() => validateOccupancyDates(date))
    }
    validateOccupancyDates('2024-02-29')
  })
})
