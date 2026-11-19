import { Head, Link, useForm } from '@inertiajs/react'
import { ArrowLeft, Send } from 'lucide-react'
import Shell from '../../components/shell'

interface Props {
  announcement: {
    id: number
    subject: string
    body: string
    status: 'draft' | 'queued'
    createdAt: string
  }
  units: { id: number; label: string }[]
  jobs: {
    id: number
    name: string
    email: string
    unitLabels: string
    status: 'pending' | 'sending' | 'sent' | 'failed'
    attempts: number
    lastError: string | null
    sentAt: string | null
  }[]
}

const statusLabel = {
  pending: 'Pendente',
  sending: 'A enviar',
  sent: 'Enviado',
  failed: 'Falhou',
} as const
const statusClass = {
  pending: 'neutral',
  sending: 'neutral',
  sent: 'green',
  failed: 'red',
} as const

export default function ShowAnnouncement({ announcement, units, jobs }: Props) {
  const form = useForm({ unitId: '' })
  return (
    <Shell view="announcements">
      <Head title={announcement.subject} />
      <div className="page-heading">
        <div>
          <Link href="/announcements" className="muted small-text">
            <ArrowLeft size={14} /> Comunicados
          </Link>
          <h1>{announcement.subject}</h1>
          <p className="muted">{announcement.status === 'queued' ? 'Em fila' : 'Rascunho'}</p>
        </div>
      </div>
      <p style={{ whiteSpace: 'pre-wrap' }}>{announcement.body}</p>

      {announcement.status === 'draft' ? (
        <form
          onSubmit={(event) => {
            event.preventDefault()
            if (
              window.confirm(
                'Os destinatários são fixados agora e a mensagem fica em fila para envio. Continuar?'
              )
            ) {
              form.transform((data) => (data.unitId ? { unitId: Number(data.unitId) } : {}))
              form.post(`/announcements/${announcement.id}/queue`)
            }
          }}
        >
          <label>
            Destinatários
            <select
              value={form.data.unitId}
              onChange={(event) => form.setData('unitId', event.target.value)}
            >
              <option value="">Todos os residentes ativos com email</option>
              {units.map((unit) => (
                <option key={unit.id} value={unit.id}>
                  Apenas a fração {unit.label}
                </option>
              ))}
            </select>
          </label>
          <button className="button primary" disabled={form.processing}>
            <Send size={17} />
            Colocar em fila
          </button>
        </form>
      ) : (
        <>
          <div className="section-heading">
            <h2>Destinatários</h2>
            <span className="count">{jobs.length}</span>
          </div>
          <p className="muted small-text">
            O envio é feito pelo comando <code>node ace send:emails</code>.
          </p>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Nome</th>
                  <th>Email</th>
                  <th>Frações</th>
                  <th>Estado</th>
                  <th>Tentativas</th>
                </tr>
              </thead>
              <tbody>
                {jobs.map((job) => (
                  <tr key={job.id}>
                    <td>{job.name}</td>
                    <td>{job.email}</td>
                    <td>{job.unitLabels}</td>
                    <td>
                      <span
                        className={`badge ${statusClass[job.status]}`}
                        title={job.lastError ?? undefined}
                      >
                        {statusLabel[job.status]}
                      </span>
                    </td>
                    <td>{job.attempts}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </Shell>
  )
}
