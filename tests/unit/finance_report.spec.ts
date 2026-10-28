import { test } from '@japa/runner'
import { financialReport, reportClipboard } from '../../inertia/lib/finance_report.js'
import { money, type BankTransaction, type Charge } from '../../inertia/lib/finances.js'

test.group('Current financial report', () => {
  const base: BankTransaction = {
    id: 1,
    date: '2025-01-03',
    value_date: '2025-01-03',
    description: 'Bank fee',
    amount: -750,
    balance: 492211,
    category: 'recurring',
    tag_id: null,
    tag_name: null,
    resident_id: null,
    unit_id: null,
    resident_name: null,
    unit_label: null,
  }
  const charges: Charge[] = [
    {
      id: 1,
      resident_id: 1,
      unit_id: 1,
      name: 'Owner <script>alert(1)</script>',
      unit_label: '1A & 1B',
      description: 'January',
      due_date: '2025-01-31',
      amount: 10000,
    },
    {
      id: 2,
      resident_id: 1,
      unit_id: 1,
      name: 'Owner <script>alert(1)</script>',
      unit_label: '1A & 1B',
      description: 'March',
      due_date: '2025-03-31',
      amount: 10000,
    },
  ]

  test('summarizes all recorded data as of today and excludes future bank payments', ({
    assert,
  }) => {
    const transactions = [
      base,
      { ...base, id: 2, date: '2025-02-01', amount: 4000, balance: 496211 },
      { ...base, id: 3, date: '2025-03-01', amount: 6000, balance: 502211 },
    ]
    const report = financialReport(
      transactions,
      charges,
      [
        { id: 1, transaction_id: 2, charge_id: 1, amount: 4000 },
        { id: 2, transaction_id: 3, charge_id: 1, amount: 6000 },
      ],
      '2025-02-01'
    )
    const totals = new Map(report.tables[0].rows.map(([label, amount]) => [label, amount]))
    assert.equal(report.statementDate, '2025-02-01')
    assert.equal(totals.get('Saldo do último extrato'), money(496211))
    assert.equal(totals.get('Entradas registadas'), money(4000))
    assert.equal(totals.get('Saídas registadas'), money(750))
    assert.equal(totals.get('Quotas por liquidar'), money(16000))
    assert.equal(totals.get('Dívida vencida'), money(6000))
    assert.deepEqual(report.tables[3].rows[0].slice(2), [money(16000), money(6000)])
    assert.equal(transactions[0].id, 1)
  })

  test('creates Word-compatible HTML tables and plain text while escaping imported names', ({
    assert,
  }) => {
    const report = financialReport(
      [{ ...base, description: '<img src=x onerror=alert(1)>', tag_name: 'Lift & "repairs"' }],
      charges,
      [],
      '2025-02-01'
    )
    const clipboard = reportClipboard(report)
    assert.include(clipboard.html, '<table')
    assert.include(clipboard.html, '<th style=')
    assert.include(clipboard.html, 'Owner &lt;script&gt;')
    assert.notInclude(clipboard.html, '<script>')
    assert.include(clipboard.html, 'Lift &amp; &quot;repairs&quot;')
    assert.include(clipboard.text, 'Residente\tFração\tPor liquidar\tVencido')
    assert.include(clipboard.text, '01/02/2025')
    assert.include(clipboard.text, '03/01/2025')
  })

  test('handles an empty condominium without inventing a bank balance', ({ assert }) => {
    const report = financialReport([], [], [], '2025-02-01')
    assert.isNull(report.statementDate)
    assert.equal(report.tables[0].rows[0][1], 'Sem extratos')
    assert.include(reportClipboard(report).text, 'Sem registos.')
  })
})
