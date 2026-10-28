import { useState } from 'react'
import { AlertCircle, CheckCircle2, Copy, LoaderCircle } from 'lucide-react'
import Modal from './modal'
import { financialReport, reportClipboard } from '../lib/finance_report'
import { date, today } from '../lib/types'
import type { BankTransaction, Charge, Payment } from '../lib/finances'

export default function FinanceReport({
  transactions,
  charges,
  payments,
  onClose,
}: {
  transactions: BankTransaction[]
  charges: Charge[]
  payments: Payment[]
  onClose: () => void
}) {
  const [copying, setCopying] = useState(false)
  const [notice, setNotice] = useState<{ kind: 'success' | 'error'; text: string } | null>(null)
  const report = financialReport(transactions, charges, payments, today())

  async function copy() {
    setCopying(true)
    setNotice(null)
    const { html, text } = reportClipboard(report)
    try {
      if (navigator.clipboard?.write && typeof ClipboardItem !== 'undefined') {
        try {
          await navigator.clipboard.write([
            new ClipboardItem({
              'text/html': new Blob([html], { type: 'text/html' }),
              'text/plain': new Blob([text], { type: 'text/plain' }),
            }),
          ])
          setNotice({ kind: 'success', text: 'Resumo copiado com tabelas formatadas.' })
          return
        } catch {
          if (!navigator.clipboard.writeText) throw new Error('Clipboard unavailable')
        }
      }
      if (!navigator.clipboard?.writeText) throw new Error('Clipboard unavailable')
      await navigator.clipboard.writeText(text)
      setNotice({ kind: 'success', text: 'Resumo copiado em texto simples.' })
    } catch {
      setNotice({
        kind: 'error',
        text: 'Não foi possível copiar. Verifique a permissão de acesso à área de transferência e tente novamente.',
      })
    } finally {
      setCopying(false)
    }
  }

  return (
    <Modal title="Resumo financeiro" onClose={onClose}>
      <div className="finance-report">
        <div className="finance-report-heading">
          <div>
            <h3>Situação financeira do condomínio</h3>
            <p>Data de referência: {date(report.asOf)}</p>
          </div>
          <button className="button primary" onClick={copy} disabled={copying}>
            {copying ? <LoaderCircle size={16} /> : <Copy size={16} />}
            {copying ? 'A copiar...' : 'Copiar para Word'}
          </button>
        </div>
        <p className="muted small-text">
          Movimentos importados até {date(report.asOf)}. Quotas registadas, incluindo vencimentos
          futuros.
        </p>
        <p className="muted small-text">
          {report.statementDate
            ? `Saldo bancário conforme extrato de ${date(report.statementDate)}.`
            : 'Sem extratos bancários importados.'}
        </p>
        {notice && (
          <div
            className={`notice ${notice.kind} finance-copy-notice`}
            role={notice.kind === 'error' ? 'alert' : 'status'}
          >
            {notice.kind === 'error' ? <AlertCircle size={18} /> : <CheckCircle2 size={18} />}
            {notice.text}
          </div>
        )}
        {report.tables.map((table) => (
          <section key={table.title} className="finance-report-section">
            <h3>{table.title}</h3>
            {table.rows.length ? (
              <div className="table-wrap">
                <table className={table.headers.length === 2 ? 'finance-report-totals' : undefined}>
                  <thead>
                    <tr>
                      {table.headers.map((header) => (
                        <th key={header}>{header}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {table.rows.map((row, index) => (
                      <tr key={index}>
                        {row.map((cell, cellIndex) => (
                          <td key={cellIndex}>{cell}</td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="muted">Sem registos.</p>
            )}
          </section>
        ))}
        <div className="form-actions">
          <button className="button secondary" onClick={onClose}>
            Fechar
          </button>
        </div>
      </div>
    </Modal>
  )
}
