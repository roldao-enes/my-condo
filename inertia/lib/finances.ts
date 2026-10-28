export type ExpenseCategory = 'unclassified' | 'recurring' | 'extraordinary'
export interface BankTransaction {
  id: number
  date: string
  value_date: string
  description: string
  amount: number
  balance: number
  category: ExpenseCategory
  tag_id: number | null
  tag_name: string | null
  resident_id: number | null
  unit_id: number | null
  resident_name: string | null
  unit_label: string | null
}
export interface FinanceTag {
  id: number
  name: string
  pattern: string
  match_type: 'exact' | 'contains'
  category: 'recurring' | 'extraordinary' | 'resident_payment'
  resident_id: number | null
  unit_id: number | null
  resident_name: string | null
  unit_label: string | null
}
export const tagCategories = {
  recurring: 'Recorrente',
  extraordinary: 'Extraordinária',
  resident_payment: 'Pagamento de residente',
}
export interface Account {
  resident_id: number
  unit_id: number
  name: string
  unit_label: string
}
export interface Charge extends Account {
  id: number
  description: string
  due_date: string
  amount: number
}
export interface Payment {
  id: number
  transaction_id: number
  charge_id: number
  amount: number
}
export const categories: Record<ExpenseCategory, string> = {
  unclassified: 'Por classificar',
  recurring: 'Recorrente',
  extraordinary: 'Extraordinária',
}
export const money = (cents: number) =>
  new Intl.NumberFormat('pt-PT', { style: 'currency', currency: 'EUR' }).format(cents / 100)
export const amountInput = (cents: number) => (cents / 100).toFixed(2).replace('.', ',')

export function financeBalances(charges: Charge[], payments: Payment[], today: string) {
  const balances = charges.map((charge) => {
    const paid = payments
      .filter((payment) => payment.charge_id === charge.id)
      .reduce((sum, payment) => sum + payment.amount, 0)
    const outstanding = charge.amount - paid
    return { ...charge, paid, outstanding, overdue: charge.due_date < today ? outstanding : 0 }
  })
  const accounts = new Map<string, Account & { outstanding: number; overdue: number }>()
  for (const charge of balances) {
    const key = `${charge.resident_id}:${charge.unit_id}`
    const account = accounts.get(key) || { ...charge, outstanding: 0, overdue: 0 }
    account.outstanding += charge.outstanding
    account.overdue += charge.overdue
    accounts.set(key, account)
  }
  return { charges: balances, accounts: [...accounts.values()] }
}
