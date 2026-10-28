import { test } from '@japa/runner'
import { readFile } from 'node:fs/promises'
import { parseMoney, parseStatement } from '../../app/finances/domain/statement.js'

test.group('Bank statement parsing', () => {
  test('imports the supplied Portuguese bank statement without losing cents', async ({
    assert,
  }) => {
    const rows = parseStatement(await readFile('tests/fixtures/bank_statement.tsv', 'utf8'))
    assert.lengthOf(rows, 23)
    assert.equal(rows[0].amount, -750)
    assert.equal(rows[0].balance, 492211)
    assert.equal(rows[12].amount, 21400)
    assert.equal(rows.at(-1)!.balance, 491890)
    assert.equal(
      rows.reduce((total, row) => total + row.amount, 0),
      -1071
    )
  })

  test('supports headers, BOM, quoted fields and semicolon exports', ({ assert }) => {
    const rows = parseStatement(
      '\uFEFFData;Data valor;Descricao;Montante;Saldo\n03/01/2025;03/01/2025;"Fornecedor; luz";-7,50;4.922,11'
    )
    assert.equal(rows[0].date, '2025-01-03')
    assert.equal(rows[0].description, 'Fornecedor; luz')
    assert.equal(parseMoney('117,68'), 11768)
  })

  test('rejects invalid dates, amounts, empty statements and malformed rows', ({ assert }) => {
    for (const content of [
      '',
      '2025-02-30\t2025-01-01\tFee\t-1\t2',
      '2025-01-01\t2025-01-01\tFee\t1x\t2',
      '2025-01-01\tFee\t1',
    ]) {
      assert.throws(() => parseStatement(content))
    }
    assert.throws(() => parseMoney('1,001'))
  })
})
