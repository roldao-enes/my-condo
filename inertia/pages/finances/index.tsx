import { Head, useForm } from '@inertiajs/react'
import { useState, type FormEvent } from 'react'
import {
  ArrowDownLeft,
  ArrowUpRight,
  CircleDollarSign,
  FileUp,
  Plus,
  Search,
  Undo2,
  Wallet,
  Tag,
  UserRoundCheck,
  FileText,
} from 'lucide-react'
import Shell from '../../components/shell'
import Modal from '../../components/modal'
import TagRegistry from '../../components/finance_tags'
import FinanceReport from '../../components/finance_report'
import { date, today } from '../../lib/types'
import {
  amountInput,
  categories,
  financeBalances,
  money,
  type Account,
  type BankTransaction,
  type Charge,
  type ExpenseCategory,
  type Payment,
  type FinanceTag,
} from '../../lib/finances'

interface Props {
  transactions: BankTransaction[]
  charges: Charge[]
  payments: Payment[]
  accounts: Account[]
  tags: FinanceTag[]
}
type View = 'transactions' | 'expenses' | 'debt' | 'tags'

export default function Finances({ transactions, charges, payments, accounts, tags }: Props) {
  const [view, setView] = useState<View>('transactions')
  const [month, setMonth] = useState('')
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState('all')
  const [dialog, setDialog] = useState<'import' | 'charge' | 'report' | BankTransaction | null>(
    null
  )
  const balances = financeBalances(charges, payments, today())
  const period = transactions.filter((transaction) => !month || transaction.date.startsWith(month))
  const income = period.reduce((sum, transaction) => sum + Math.max(0, transaction.amount), 0)
  const expenses = period.reduce((sum, transaction) => sum - Math.min(0, transaction.amount), 0)
  const overdue = balances.charges.reduce((sum, charge) => sum + charge.overdue, 0)
  const outstanding = balances.charges.reduce((sum, charge) => sum + charge.outstanding, 0)
  const available = (transaction: BankTransaction) =>
    transaction.amount -
    payments
      .filter((payment) => payment.transaction_id === transaction.id)
      .reduce((sum, payment) => sum + payment.amount, 0)
  const visible = period.filter(
    (transaction) =>
      (view !== 'expenses' || transaction.amount < 0) &&
      (category === 'all' ||
        (category === 'unmatched'
          ? transaction.amount > 0 && available(transaction) > 0
          : category === 'resident_payment'
            ? transaction.amount > 0 && Boolean(transaction.resident_id)
            : transaction.amount < 0 && transaction.category === category)) &&
      `${transaction.description} ${transaction.tag_name || ''} ${transaction.resident_name || ''}`
        .toLocaleLowerCase('pt-PT')
        .includes(query.toLocaleLowerCase('pt-PT'))
  )
  const visibleCharges = balances.charges.filter((charge) =>
    `${charge.name} ${charge.unit_label} ${charge.description}`
      .toLocaleLowerCase('pt-PT')
      .includes(query.toLocaleLowerCase('pt-PT'))
  )
  const close = () => setDialog(null)

  return (
    <Shell view="finances">
      <Head title="Finanças" />
      <div className="page-heading">
        <div>
          <div className="eyebrow">GESTÃO DO CONDOMÍNIO</div>
          <h1>Finanças</h1>
          <p className="muted">
            {transactions.length} movimentos · {charges.length} quotas registadas
          </p>
        </div>
        <div className="heading-actions">
          <button className="button secondary" onClick={() => setDialog('report')}>
            <FileText size={17} />
            Resumo financeiro
          </button>
          <button
            className="button secondary"
            onClick={() => setDialog('charge')}
            disabled={!accounts.length}
          >
            <Plus size={17} />
            Registar quota
          </button>
          <button className="button primary" onClick={() => setDialog('import')}>
            <FileUp size={17} />
            Importar extrato
          </button>
        </div>
      </div>
      <div className="finance-period">
        <label>
          Período dos movimentos
          <select value={month} onChange={(event) => setMonth(event.target.value)}>
            <option value="">Todo o histórico</option>
            {[...new Set(transactions.map((transaction) => transaction.date.slice(0, 7)))].map(
              (value) => (
                <option key={value} value={value}>
                  {value.slice(5)}/{value.slice(0, 4)}
                </option>
              )
            )}
          </select>
        </label>
        <span className="muted small-text">
          {transactions[0]
            ? `Último extrato: ${date(transactions[0].date)}`
            : 'Sem extratos importados'}
        </span>
      </div>
      <div className="stats-grid finance-stats">
        <Metric
          label="Saldo do último extrato"
          value={transactions[0] ? money(transactions[0].balance) : '—'}
          icon={Wallet}
          tone="green"
          caption={transactions[0] ? date(transactions[0].date) : 'Sem saldo bancário'}
        />
        <Metric
          label="Entradas no período"
          value={money(income)}
          icon={ArrowDownLeft}
          tone="blue"
          caption={`${period.filter((transaction) => transaction.amount > 0).length} movimentos`}
        />
        <Metric
          label="Saídas no período"
          value={money(expenses)}
          icon={ArrowUpRight}
          tone="rose"
          caption={`${period.filter((transaction) => transaction.amount < 0).length} movimentos`}
        />
        <Metric
          label="Dívida vencida · hoje"
          value={money(overdue)}
          icon={CircleDollarSign}
          tone="rose"
          caption={`${money(outstanding)} por liquidar no total`}
        />
      </div>
      <div className="finance-breakdown">
        {(Object.entries(categories) as [ExpenseCategory, string][]).map(([key, label]) => {
          const total = period
            .filter((transaction) => transaction.amount < 0 && transaction.category === key)
            .reduce((sum, transaction) => sum - transaction.amount, 0)
          return (
            <div key={key}>
              <span className={`finance-dot ${key}`} />
              <span>{label}</span>
              <strong>{money(total)}</strong>
              <progress aria-label={`Despesas: ${label}`} value={total} max={expenses || 1} />
            </div>
          )
        })}
      </div>
      <div className="finance-toolbar">
        <div className="segments" role="tablist" aria-label="Finanças">
          {(
            [
              ['transactions', 'Movimentos'],
              ['expenses', 'Despesas'],
              ['debt', 'Dívida'],
              ['tags', 'Etiquetas'],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              role="tab"
              id={`tab-${key}`}
              aria-controls="finance-panel"
              aria-selected={view === key}
              className={view === key ? 'selected' : ''}
              onClick={() => {
                setView(key)
                setCategory('all')
                setQuery('')
              }}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="search-box">
          <Search size={16} />
          <input
            aria-label="Pesquisar finanças"
            placeholder={
              view === 'tags'
                ? 'Pesquisar nome ou etiqueta...'
                : view === 'debt'
                  ? 'Pesquisar residente ou quota...'
                  : 'Pesquisar movimento...'
            }
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        {(view === 'transactions' || view === 'expenses') && (
          <select
            className="finance-select"
            aria-label="Filtrar classificação"
            value={category}
            onChange={(event) => setCategory(event.target.value)}
          >
            <option value="all">Todas as classificações</option>
            {Object.entries(categories).map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
            {view === 'transactions' && <option value="unmatched">Entradas por associar</option>}
            {view === 'transactions' && (
              <option value="resident_payment">Pagamentos de residentes</option>
            )}
          </select>
        )}
      </div>
      <section id="finance-panel" role="tabpanel" aria-labelledby={`tab-${view}`}>
        {view === 'tags' ? (
          <TagRegistry tags={tags} accounts={accounts} query={query} />
        ) : view === 'debt' ? (
          <>
            <div className="section-heading">
              <h2>Saldos por residente</h2>
              <span className="muted small-text">Todas as quotas · {date(today())}</span>
            </div>
            {balances.accounts.length ? (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Residente</th>
                      <th>Fração</th>
                      <th>Por liquidar</th>
                      <th>Vencido</th>
                    </tr>
                  </thead>
                  <tbody>
                    {balances.accounts
                      .filter((account) =>
                        `${account.name} ${account.unit_label}`
                          .toLocaleLowerCase('pt-PT')
                          .includes(query.toLocaleLowerCase('pt-PT'))
                      )
                      .map((account) => (
                        <tr key={`${account.resident_id}:${account.unit_id}`}>
                          <td>{account.name}</td>
                          <td>{account.unit_label}</td>
                          <td className="finance-number">{money(account.outstanding)}</td>
                          <td
                            className={`finance-number ${account.overdue ? 'finance-negative' : ''}`}
                          >
                            {money(account.overdue)}
                          </td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <Empty text="Ainda não existem quotas registadas." />
            )}
            <div className="section-heading activity-heading">
              <h2>Quotas</h2>
              <span className="count">{visibleCharges.length}</span>
            </div>
            {visibleCharges.length ? (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Residente / fração</th>
                      <th>Quota</th>
                      <th>Vencimento</th>
                      <th>Valor</th>
                      <th>Pago</th>
                      <th>Por liquidar</th>
                      <th>Estado</th>
                    </tr>
                  </thead>
                  <tbody>
                    {visibleCharges.map((charge) => (
                      <tr key={charge.id}>
                        <td>
                          <strong>{charge.name}</strong>
                          <small className="finance-subtext">{charge.unit_label}</small>
                        </td>
                        <td className="finance-description">{charge.description}</td>
                        <td>{date(charge.due_date)}</td>
                        <td className="finance-number">{money(charge.amount)}</td>
                        <td className="finance-number">{money(charge.paid)}</td>
                        <td className="finance-number">{money(charge.outstanding)}</td>
                        <td>
                          <span
                            className={`badge ${charge.outstanding === 0 ? 'green' : charge.overdue ? 'finance-overdue' : 'neutral'}`}
                          >
                            {charge.outstanding === 0
                              ? 'Liquidada'
                              : charge.overdue
                                ? 'Vencida'
                                : 'Pendente'}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              charges.length > 0 && <Empty text="Nenhuma quota encontrada." />
            )}
          </>
        ) : visible.length ? (
          <>
            <div className="section-heading">
              <h2>
                {view === 'expenses' ? 'Despesas' : 'Movimentos bancários'}{' '}
                <span className="count">{visible.length}</span>
              </h2>
            </div>
            <div className="table-wrap">
              <table className="finance-transactions">
                <thead>
                  <tr>
                    <th>Data / valor</th>
                    <th>Descrição</th>
                    <th>Montante</th>
                    <th>Saldo</th>
                    <th>{view === 'expenses' ? 'Classificação' : 'Classificação / pagamento'}</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map((transaction) => (
                    <tr key={transaction.id}>
                      <td>
                        {date(transaction.date)}
                        <small className="finance-subtext">{date(transaction.value_date)}</small>
                      </td>
                      <td className="finance-description">
                        {transaction.description}
                        {transaction.tag_name && (
                          <small className="finance-tag-label">
                            <Tag size={12} />
                            {transaction.tag_name}
                          </small>
                        )}
                      </td>
                      <td
                        className={`finance-number ${transaction.amount < 0 ? 'finance-negative' : 'finance-positive'}`}
                      >
                        {transaction.amount > 0 ? '+' : ''}
                        {money(transaction.amount)}
                      </td>
                      <td className="finance-number">{money(transaction.balance)}</td>
                      <td>
                        {transaction.amount < 0 ? (
                          <CategorySelect transaction={transaction} />
                        ) : (
                          <>
                            <small className="finance-resident-name">
                              {transaction.resident_name
                                ? `${transaction.resident_name} · ${transaction.unit_label}`
                                : 'Residente por identificar'}
                            </small>
                            <button
                              className="button secondary"
                              onClick={() => setDialog(transaction)}
                            >
                              <CircleDollarSign size={15} />
                              {available(transaction)
                                ? `${money(available(transaction))} por associar`
                                : 'Associado'}
                            </button>
                          </>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        ) : (
          <Empty
            text={
              transactions.length
                ? 'Nenhum movimento encontrado.'
                : 'Ainda não existem movimentos bancários.'
            }
          />
        )}
      </section>
      {dialog === 'import' && <ImportDialog onClose={close} />}
      {dialog === 'report' && (
        <FinanceReport
          transactions={transactions}
          charges={charges}
          payments={payments}
          onClose={close}
        />
      )}
      {dialog === 'charge' && <ChargeDialog accounts={accounts} onClose={close} />}
      {dialog && typeof dialog === 'object' && (
        <PaymentDialog
          transaction={transactions.find((transaction) => transaction.id === dialog.id) || dialog}
          charges={balances.charges}
          payments={payments}
          accounts={accounts}
          onClose={close}
        />
      )}
    </Shell>
  )
}

function Metric({
  label,
  value,
  caption,
  icon: Icon,
  tone,
}: {
  label: string
  value: string
  caption: string
  icon: typeof Wallet
  tone: string
}) {
  return (
    <div className="stat">
      <div className="stat-top">
        <span>{label}</span>
        <span className={`stat-icon ${tone}`}>
          <Icon size={18} />
        </span>
      </div>
      <strong>{value}</strong>
      <small>{caption}</small>
    </div>
  )
}
function Empty({ text }: { text: string }) {
  return (
    <div className="quiet-empty">
      <Wallet size={20} />
      {text}
    </div>
  )
}
function CategorySelect({ transaction }: { transaction: BankTransaction }) {
  const form = useForm({ category: transaction.category, remember: true })
  return (
    <div>
      <select
        className="finance-select"
        aria-label={`Classificação de ${transaction.description}`}
        value={transaction.category}
        disabled={form.processing}
        onChange={(event) => {
          form.transform(() => ({
            category: event.target.value as ExpenseCategory,
            remember: form.data.remember,
          }))
          form.post(`/finances/transactions/${transaction.id}/category`, { preserveScroll: true })
        }}
      >
        {Object.entries(categories).map(([key, label]) => (
          <option key={key} value={key}>
            {label}
          </option>
        ))}
      </select>
      <label className="finance-remember">
        <input
          type="checkbox"
          checked={form.data.remember}
          onChange={(event) => form.setData('remember', event.target.checked)}
        />
        Memorizar nome
      </label>
    </div>
  )
}
function ImportDialog({ onClose }: { onClose: () => void }) {
  const form = useForm<{ statement: File | null }>({ statement: null })
  return (
    <Modal title="Importar extrato" onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          form.post('/finances/import', {
            forceFormData: true,
            onSuccess: (page) => {
              if (!(page.props.flash as { error?: string })?.error) onClose()
            },
          })
        }}
      >
        <label>
          Extrato bancário (.csv)
          <input
            type="file"
            name="statement"
            accept=".csv,.tsv,.txt"
            required
            onChange={(event) => form.setData('statement', event.target.files?.[0] || null)}
          />
        </label>
        {form.progress && (
          <progress
            className="finance-upload-progress"
            aria-label="Progresso da importação"
            value={form.progress.percentage}
            max={100}
          />
        )}
        <div className="form-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            Cancelar
          </button>
          <button className="button primary" disabled={form.processing || !form.data.statement}>
            <FileUp size={16} />
            {form.processing ? 'A importar...' : 'Importar'}
          </button>
        </div>
      </form>
    </Modal>
  )
}
function ChargeDialog({ accounts, onClose }: { accounts: Account[]; onClose: () => void }) {
  const [accountKey, setAccountKey] = useState('')
  const form = useForm({ residentId: 0, unitId: 0, description: '', dueDate: today(), amount: '' })
  function submit(event: FormEvent) {
    event.preventDefault()
    form.post('/finances/charges', {
      preserveScroll: true,
      onSuccess: (page) => {
        if (!(page.props.flash as { error?: string })?.error) onClose()
      },
    })
  }
  return (
    <Modal title="Registar quota" onClose={onClose}>
      <form onSubmit={submit}>
        <label>
          Residente / fração
          <select
            required
            value={accountKey}
            onChange={(event) => {
              setAccountKey(event.target.value)
              const account = accounts.find(
                (item) => `${item.resident_id}:${item.unit_id}` === event.target.value
              )
              if (account)
                form.setData({
                  ...form.data,
                  residentId: account.resident_id,
                  unitId: account.unit_id,
                })
            }}
          >
            <option value="">Selecionar residente</option>
            {accounts.map((account) => (
              <option
                key={`${account.resident_id}:${account.unit_id}`}
                value={`${account.resident_id}:${account.unit_id}`}
              >
                {account.name} · {account.unit_label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Descrição
          <input
            required
            maxLength={200}
            value={form.data.description}
            onChange={(event) => form.setData('description', event.target.value)}
          />
        </label>
        <div className="form-grid">
          <label>
            Vencimento
            <input
              type="date"
              required
              value={form.data.dueDate}
              onChange={(event) => form.setData('dueDate', event.target.value)}
            />
          </label>
          <label>
            Montante (EUR)
            <input
              inputMode="decimal"
              required
              value={form.data.amount}
              onChange={(event) => form.setData('amount', event.target.value)}
            />
          </label>
        </div>
        <div className="form-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            Cancelar
          </button>
          <button className="button primary" disabled={form.processing}>
            <Plus size={16} />
            {form.processing ? 'A registar...' : 'Registar quota'}
          </button>
        </div>
      </form>
    </Modal>
  )
}
function PaymentDialog({
  transaction,
  charges,
  payments,
  accounts,
  onClose,
}: {
  transaction: BankTransaction
  charges: ReturnType<typeof financeBalances>['charges']
  payments: Payment[]
  accounts: Account[]
  onClose: () => void
}) {
  const matches = payments.filter((payment) => payment.transaction_id === transaction.id)
  const available = transaction.amount - matches.reduce((sum, payment) => sum + payment.amount, 0)
  const form = useForm({ chargeId: '', amount: '', remember: true })
  const remove = useForm({})
  return (
    <Modal title="Associar pagamento" onClose={onClose}>
      <div className="finance-payment-summary">
        <strong>{transaction.description}</strong>
        <span>
          {date(transaction.date)} · {money(transaction.amount)}
        </span>
        <span className="muted">Por associar: {money(available)}</span>
      </div>
      <ResidentIdentity
        key={`${transaction.id}:${transaction.resident_id}:${transaction.unit_id}`}
        transaction={transaction}
        accounts={accounts}
      />
      {matches.map((payment) => {
        const charge = charges.find((item) => item.id === payment.charge_id)!
        return (
          <div className="finance-match" key={payment.id}>
            <div>
              <strong>
                {charge.name} · {charge.unit_label}
              </strong>
              <small>
                {charge.description} · {money(payment.amount)}
              </small>
            </div>
            <button
              className="icon-button"
              title="Remover associação"
              aria-label={`Remover associação: ${charge.description}`}
              disabled={remove.processing}
              onClick={() =>
                remove.post(`/finances/payments/${payment.id}/remove`, { preserveScroll: true })
              }
            >
              <Undo2 size={17} />
            </button>
          </div>
        )
      })}
      {available > 0 && (
        <form
          onSubmit={(event) => {
            event.preventDefault()
            form.post(`/finances/transactions/${transaction.id}/payments`, {
              preserveScroll: true,
              onSuccess: () => form.reset(),
            })
          }}
        >
          <label>
            Quota
            <select
              required
              value={form.data.chargeId}
              onChange={(event) => {
                const charge = charges.find((item) => item.id === Number(event.target.value))
                form.setData({
                  ...form.data,
                  chargeId: event.target.value,
                  amount: charge ? amountInput(Math.min(available, charge.outstanding)) : '',
                })
              }}
            >
              <option value="">Selecionar quota</option>
              {charges
                .filter((charge) => charge.outstanding > 0)
                .sort(
                  (first, second) =>
                    Number(
                      second.resident_id === transaction.resident_id &&
                        second.unit_id === transaction.unit_id
                    ) -
                    Number(
                      first.resident_id === transaction.resident_id &&
                        first.unit_id === transaction.unit_id
                    )
                )
                .map((charge) => (
                  <option key={charge.id} value={charge.id}>
                    {charge.name} · {charge.unit_label} · {charge.description} ·{' '}
                    {money(charge.outstanding)}
                  </option>
                ))}
            </select>
          </label>
          <label>
            Montante (EUR)
            <input
              required
              inputMode="decimal"
              value={form.data.amount}
              onChange={(event) => form.setData('amount', event.target.value)}
            />
          </label>
          <label className="finance-remember">
            <input
              type="checkbox"
              checked={form.data.remember}
              onChange={(event) => form.setData('remember', event.target.checked)}
            />
            Memorizar nome do residente
          </label>
          <div className="form-actions">
            <button type="button" className="button secondary" onClick={onClose}>
              Fechar
            </button>
            <button
              className="button primary"
              disabled={form.processing || !form.data.chargeId || remove.processing}
            >
              <CircleDollarSign size={16} />
              {form.processing ? 'A associar...' : 'Associar'}
            </button>
          </div>
        </form>
      )}
    </Modal>
  )
}

function ResidentIdentity({
  transaction,
  accounts,
}: {
  transaction: BankTransaction
  accounts: Account[]
}) {
  const form = useForm({
    residentId: transaction.resident_id,
    unitId: transaction.unit_id,
    remember: true,
  })
  return (
    <form
      className="finance-identity"
      onSubmit={(event) => {
        event.preventDefault()
        form.post(`/finances/transactions/${transaction.id}/resident`, { preserveScroll: true })
      }}
    >
      <label>
        Residente / fração
        <select
          value={form.data.residentId ? `${form.data.residentId}:${form.data.unitId}` : ''}
          onChange={(event) => {
            const account = accounts.find(
              (item) => `${item.resident_id}:${item.unit_id}` === event.target.value
            )
            form.setData({
              ...form.data,
              residentId: account?.resident_id || null,
              unitId: account?.unit_id || null,
            })
          }}
        >
          <option value="">Sem identificação</option>
          {accounts.map((account) => (
            <option
              key={`${account.resident_id}:${account.unit_id}`}
              value={`${account.resident_id}:${account.unit_id}`}
            >
              {account.name} · {account.unit_label}
            </option>
          ))}
        </select>
      </label>
      <label className="finance-remember">
        <input
          type="checkbox"
          checked={form.data.remember}
          onChange={(event) => form.setData('remember', event.target.checked)}
        />
        Memorizar nome
      </label>
      <button className="button secondary" disabled={form.processing}>
        <UserRoundCheck size={16} />
        {form.processing ? 'A guardar...' : 'Guardar residente'}
      </button>
    </form>
  )
}
