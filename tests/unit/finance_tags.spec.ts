import { test } from '@japa/runner'
import { matchTag, normalizeName, type FinanceTag } from '../../app/finances/domain/tags.js'

test.group('Finance name registry', () => {
  const tag: FinanceTag = {
    id: 1,
    name: 'Maintenance',
    pattern: 'MARINA SEQUEIRA',
    match_type: 'contains',
    category: 'recurring',
    resident_id: null,
    unit_id: null,
  }

  test('matches normalized names with accents, punctuation and changing references', ({
    assert,
  }) => {
    assert.equal(normalizeName('  TRF.P/ Mária   Silva '), 'TRF P MARIA SILVA')
    assert.equal(matchTag('TRF.P/ Marina Sequeira REF 123', -7500, [tag])?.id, 1)
    assert.isNull(matchTag('TRF.P/ Marina Sequeiraa', -7500, [tag]))
    assert.isNull(matchTag('TRF.P/ Marina Sequeira', 7500, [tag]))
  })

  test('prefers exact then specific names and leaves ambiguous names unclassified', ({
    assert,
  }) => {
    const exact: FinanceTag = {
      ...tag,
      id: 2,
      pattern: 'TRF P MARINA SEQUEIRA',
      match_type: 'exact',
    }
    assert.equal(matchTag('TRF.P/ Marina Sequeira', -7500, [tag, exact])?.id, 2)
    assert.equal(
      matchTag('Marina Sequeira', -7500, [{ ...tag, id: 3, pattern: 'MARINA' }, tag])?.id,
      1
    )
    assert.isNull(
      matchTag('Marina Sequeira Maria Oliveira', -7500, [
        tag,
        { ...tag, id: 4, pattern: 'MARIA OLIVEIRA' },
      ])
    )
    assert.equal(
      matchTag('Marina Sequeira', 10000, [
        { ...tag, category: 'resident_payment', resident_id: 1, unit_id: 1 },
      ])?.resident_id,
      1
    )
  })
})
