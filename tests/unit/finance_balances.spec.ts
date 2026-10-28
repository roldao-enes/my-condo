import { test } from '@japa/runner'
import { financeBalances, type Charge } from '../../inertia/lib/finances.js'

test('resident debt uses only matched payments and keeps historical responsibility', ({
  assert,
}) => {
  const base = {
    resident_id: 1,
    unit_id: 1,
    name: 'Owner',
    unit_label: '1A',
    description: 'Quota',
    amount: 10000,
  }
  const charges: Charge[] = [
    { ...base, id: 1, due_date: '2025-01-31' },
    { ...base, id: 2, due_date: '2025-03-31' },
    { ...base, resident_id: 2, name: 'New owner', id: 3, due_date: '2025-02-01' },
  ]
  const balances = financeBalances(
    charges,
    [{ id: 1, transaction_id: 1, charge_id: 1, amount: 4000 }],
    '2025-02-01'
  )
  assert.equal(balances.charges[0].outstanding, 6000)
  assert.equal(balances.charges[0].overdue, 6000)
  assert.equal(balances.charges[1].overdue, 0)
  assert.equal(balances.charges[2].overdue, 0)
  assert.lengthOf(balances.accounts, 2)
  assert.equal(balances.accounts[0].outstanding, 16000)
  assert.equal(balances.accounts[1].outstanding, 10000)
})
