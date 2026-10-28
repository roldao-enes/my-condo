import {
  categories,
  financeBalances,
  money,
  type BankTransaction,
  type Charge,
  type Payment,
} from './finances.js'
import { date } from './types.js'

export interface ReportTable {
  title: string
  headers: string[]
  rows: string[][]
}
export interface FinanceReport {
  asOf: string
  statementDate: string | null
  tables: ReportTable[]
}

export function financialReport(
  transactions: BankTransaction[],
  charges: Charge[],
  payments: Payment[],
  asOf: string
): FinanceReport {
  const recorded = transactions
    .filter((transaction) => transaction.date <= asOf)
    .sort((first, second) => second.date.localeCompare(first.date) || second.id - first.id)
  const ids = new Set(recorded.map((transaction) => transaction.id))
  const matched = payments.filter((payment) => ids.has(payment.transaction_id))
  const balances = financeBalances(charges, matched, asOf)
  const income = recorded.reduce((sum, transaction) => sum + Math.max(0, transaction.amount), 0)
  const expenses = recorded.reduce((sum, transaction) => sum - Math.min(0, transaction.amount), 0)
  const paid = matched.reduce((sum, payment) => sum + payment.amount, 0)
  const outstanding = balances.charges.reduce((sum, charge) => sum + charge.outstanding, 0)
  const overdue = balances.charges.reduce((sum, charge) => sum + charge.overdue, 0)
  const latest = recorded[0]
  const totals: ReportTable = {
    title: 'Resumo financeiro',
    headers: ['Indicador', 'Valor'],
    rows: [
      ['Saldo do último extrato', latest ? money(latest.balance) : 'Sem extratos'],
      ['Entradas registadas', money(income)],
      ['Saídas registadas', money(expenses)],
      ['Resultado dos movimentos importados', money(income - expenses)],
      ['Pagamentos associados a quotas', money(paid)],
      ['Entradas por associar a quotas', money(income - paid)],
      ['Quotas registadas', money(charges.reduce((sum, charge) => sum + charge.amount, 0))],
      ['Quotas por liquidar', money(outstanding)],
      ['Dívida vencida', money(overdue)],
      ['Quotas por liquidar não vencidas', money(outstanding - overdue)],
    ],
  }
  const expenseTable: ReportTable = {
    title: 'Despesas por classificação',
    headers: ['Classificação', 'Valor'],
    rows: Object.entries(categories).map(([category, label]) => [
      label,
      money(
        recorded
          .filter((transaction) => transaction.amount < 0 && transaction.category === category)
          .reduce((sum, transaction) => sum - transaction.amount, 0)
      ),
    ]),
  }
  const names = new Map<string, { name: string; category: string; amount: number }>()
  for (const transaction of recorded.filter((item) => item.amount < 0)) {
    const name = transaction.tag_name || transaction.description
    const key = JSON.stringify([name, transaction.category])
    const entry = names.get(key) || { name, category: categories[transaction.category], amount: 0 }
    entry.amount -= transaction.amount
    names.set(key, entry)
  }
  const namesTable: ReportTable = {
    title: 'Despesas por nome',
    headers: ['Nome / etiqueta', 'Classificação', 'Valor'],
    rows: [...names.values()]
      .sort((first, second) => first.name.localeCompare(second.name, 'pt'))
      .map((entry) => [entry.name, entry.category, money(entry.amount)]),
  }
  const debtTable: ReportTable = {
    title: 'Saldos por residente',
    headers: ['Residente', 'Fração', 'Por liquidar', 'Vencido'],
    rows: balances.accounts
      .sort(
        (first, second) =>
          first.name.localeCompare(second.name, 'pt') ||
          first.unit_label.localeCompare(second.unit_label, 'pt')
      )
      .map((account) => [
        account.name,
        account.unit_label,
        money(account.outstanding),
        money(account.overdue),
      ]),
  }
  return {
    asOf,
    statementDate: latest?.date || null,
    tables: [totals, expenseTable, namesTable, debtTable],
  }
}

function escapeHtml(value: string) {
  return value.replace(
    /[&<>"']/g,
    (character) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character]!
  )
}

export function reportClipboard(report: FinanceReport) {
  const title = `Situação financeira do condomínio - ${date(report.asOf)}`
  const scope = `Movimentos importados até ${date(report.asOf)}. Quotas registadas, incluindo vencimentos futuros.`
  const statement = report.statementDate
    ? `Saldo bancário conforme extrato de ${date(report.statementDate)}.`
    : 'Sem extratos bancários importados.'
  const cellStyle = 'border:1px solid #999;padding:6px 8px;text-align:left;vertical-align:top;'
  const html =
    '<!DOCTYPE html><html lang="pt"><head><meta charset="utf-8"></head><body>' +
    `<h1>${escapeHtml(title)}</h1><p>${escapeHtml(scope)}</p><p>${escapeHtml(statement)}</p>` +
    report.tables
      .map(
        (table) =>
          `<h2>${escapeHtml(table.title)}</h2>` +
          (table.rows.length
            ? `<table style="border-collapse:collapse;width:100%;" border="1" cellpadding="6" cellspacing="0"><thead><tr>${table.headers.map((header) => `<th style="${cellStyle}background-color:#edf1ee;">${escapeHtml(header)}</th>`).join('')}</tr></thead><tbody>${table.rows.map((row) => `<tr>${row.map((cell) => `<td style="${cellStyle}">${escapeHtml(cell)}</td>`).join('')}</tr>`).join('')}</tbody></table>`
            : '<p>Sem registos.</p>')
      )
      .join('') +
    '</body></html>'
  const text = [
    title,
    scope,
    statement,
    ...report.tables.map((table) =>
      [
        table.title,
        table.headers.join('\t'),
        ...(table.rows.length ? table.rows.map((row) => row.join('\t')) : ['Sem registos.']),
      ].join('\n')
    ),
  ].join('\n\n')
  return { html, text }
}
