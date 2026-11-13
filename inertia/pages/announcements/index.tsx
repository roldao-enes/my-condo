import { Head, Link, useForm } from '@inertiajs/react'
import { useState } from 'react'
import { Mail, Plus } from 'lucide-react'
import Shell from '../../components/shell'
import Modal from '../../components/modal'

interface Announcement {
  id: number
  subject: string
  status: 'draft' | 'queued'
  createdAt: string
  total: number
  sent: number
  failed: number
}

const dateTime = (value: string) =>
  new Intl.DateTimeFormat('pt-PT', {
    timeZone: 'Europe/Lisbon',
    dateStyle: 'short',
    timeStyle: 'short',
  }).format(new Date(value))

export default function Announcements({ announcements }: { announcements: Announcement[] }) {
  const [creating, setCreating] = useState(false)
  return (
    <Shell view="announcements">
      <Head title="Comunicados" />
      <div className="page-heading">
        <div>
          <div className="eyebrow">GESTÃO DO CONDOMÍNIO</div>
          <h1>Comunicados</h1>
          <p className="muted">{announcements.length} comunicados</p>
        </div>
        <div className="heading-actions">
          <button className="button primary" onClick={() => setCreating(true)}>
            <Plus size={17} />
            Novo comunicado
          </button>
        </div>
      </div>
      {announcements.length ? (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Assunto</th>
                <th>Criado</th>
                <th>Estado</th>
                <th>Enviados</th>
              </tr>
            </thead>
            <tbody>
              {announcements.map((announcement) => (
                <tr key={announcement.id}>
                  <td>
                    <Link href={`/announcements/${announcement.id}`}>{announcement.subject}</Link>
                  </td>
                  <td>{dateTime(announcement.createdAt)}</td>
                  <td>
                    <span
                      className={`badge ${announcement.status === 'queued' ? 'green' : 'neutral'}`}
                    >
                      {announcement.status === 'queued' ? 'Em fila' : 'Rascunho'}
                    </span>
                  </td>
                  <td>
                    {announcement.status === 'queued'
                      ? `${announcement.sent}/${announcement.total}${announcement.failed ? ` · ${announcement.failed} falhados` : ''}`
                      : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="quiet-empty">
          <Mail size={20} />
          Ainda não existem comunicados.
        </div>
      )}
      {creating && <DraftForm onClose={() => setCreating(false)} />}
    </Shell>
  )
}

function DraftForm({ onClose }: { onClose: () => void }) {
  const form = useForm({ subject: '', body: '' })
  return (
    <Modal title="Novo comunicado" onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          form.post('/announcements')
        }}
      >
        <label>
          Assunto
          <input
            autoFocus
            required
            maxLength={200}
            value={form.data.subject}
            onChange={(event) => form.setData('subject', event.target.value)}
          />
        </label>
        <label>
          Mensagem
          <textarea
            required
            rows={8}
            maxLength={10000}
            value={form.data.body}
            onChange={(event) => form.setData('body', event.target.value)}
          />
        </label>
        <div className="form-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            Cancelar
          </button>
          <button className="button primary" disabled={form.processing}>
            Criar rascunho
          </button>
        </div>
      </form>
    </Modal>
  )
}
