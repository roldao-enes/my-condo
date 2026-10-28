import { useForm } from '@inertiajs/react'
import { useState } from 'react'
import { Pencil, Plus, Save, Tags, Trash2 } from 'lucide-react'
import Modal from './modal'
import { tagCategories, type Account, type FinanceTag } from '../lib/finances'

export default function TagRegistry({
  tags,
  accounts,
  query,
}: {
  tags: FinanceTag[]
  accounts: Account[]
  query: string
}) {
  const [dialog, setDialog] = useState<FinanceTag | 'new' | null>(null)
  const [removing, setRemoving] = useState<FinanceTag | null>(null)
  const remove = useForm({})
  const visible = tags.filter((tag) =>
    `${tag.name} ${tag.pattern} ${tag.resident_name || ''} ${tag.unit_label || ''}`
      .toLocaleLowerCase('pt-PT')
      .includes(query.toLocaleLowerCase('pt-PT'))
  )
  return (
    <>
      <div className="section-heading">
        <h2>
          Nomes e etiquetas <span className="count">{visible.length}</span>
        </h2>
        <button className="button primary" onClick={() => setDialog('new')}>
          <Plus size={16} />
          Nova etiqueta
        </button>
      </div>
      {visible.length ? (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Etiqueta</th>
                <th>Nome no extrato</th>
                <th>Correspondência</th>
                <th>Classificação</th>
                <th>Residente / fração</th>
                <th>
                  <span className="sr-only">Ações</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {visible.map((tag) => (
                <tr key={tag.id}>
                  <td className="finance-description">
                    <strong>{tag.name}</strong>
                  </td>
                  <td className="finance-description">{tag.pattern}</td>
                  <td>{tag.match_type === 'exact' ? 'Descrição completa' : 'Contém nome'}</td>
                  <td>{tagCategories[tag.category]}</td>
                  <td>{tag.resident_name ? `${tag.resident_name} · ${tag.unit_label}` : '—'}</td>
                  <td>
                    <div className="finance-tag-actions">
                      <button
                        className="icon-button"
                        title="Editar etiqueta"
                        aria-label={`Editar etiqueta: ${tag.name}`}
                        onClick={() => setDialog(tag)}
                      >
                        <Pencil size={16} />
                      </button>
                      <button
                        className="icon-button"
                        title="Remover etiqueta"
                        aria-label={`Remover etiqueta: ${tag.name}`}
                        onClick={() => setRemoving(tag)}
                      >
                        <Trash2 size={16} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="quiet-empty">
          <Tags size={20} />
          {tags.length ? 'Nenhuma etiqueta encontrada.' : 'Ainda não existem nomes registados.'}
        </div>
      )}
      {dialog && (
        <TagDialog
          tag={dialog === 'new' ? null : dialog}
          accounts={accounts}
          onClose={() => setDialog(null)}
        />
      )}
      {removing && (
        <Modal title="Remover etiqueta" onClose={() => setRemoving(null)}>
          <p>{removing.name}</p>
          <div className="form-actions">
            <button className="button secondary" onClick={() => setRemoving(null)}>
              Cancelar
            </button>
            <button
              className="button primary"
              disabled={remove.processing}
              onClick={() =>
                remove.post(`/finances/tags/${removing.id}/remove`, {
                  preserveScroll: true,
                  onSuccess: (page) => {
                    if (!(page.props.flash as { error?: string })?.error) setRemoving(null)
                  },
                })
              }
            >
              <Trash2 size={16} />
              Remover
            </button>
          </div>
        </Modal>
      )}
    </>
  )
}

function TagDialog({
  tag,
  accounts,
  onClose,
}: {
  tag: FinanceTag | null
  accounts: Account[]
  onClose: () => void
}) {
  const form = useForm({
    name: tag?.name || '',
    pattern: tag?.pattern || '',
    matchType: tag?.match_type || 'exact',
    category: tag?.category || 'recurring',
    residentId: tag?.resident_id || null,
    unitId: tag?.unit_id || null,
  })
  return (
    <Modal title={tag ? 'Editar etiqueta' : 'Nova etiqueta'} onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          form.post(tag ? `/finances/tags/${tag.id}` : '/finances/tags', {
            preserveScroll: true,
            onSuccess: (page) => {
              if (!(page.props.flash as { error?: string })?.error) onClose()
            },
          })
        }}
      >
        <label>
          Etiqueta
          <input
            required
            maxLength={160}
            value={form.data.name}
            onChange={(event) => form.setData('name', event.target.value)}
          />
        </label>
        <label>
          Nome no extrato
          <input
            required
            minLength={3}
            maxLength={500}
            value={form.data.pattern}
            onChange={(event) => form.setData('pattern', event.target.value)}
          />
        </label>
        <label>
          Correspondência
          <select
            value={form.data.matchType}
            onChange={(event) =>
              form.setData('matchType', event.target.value as 'exact' | 'contains')
            }
          >
            <option value="exact">Descrição completa</option>
            <option value="contains">Contém nome</option>
          </select>
        </label>
        <label>
          Classificação
          <select
            value={form.data.category}
            onChange={(event) =>
              form.setData('category', event.target.value as FinanceTag['category'])
            }
          >
            {Object.entries(tagCategories).map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {form.data.category === 'resident_payment' && (
          <label>
            Residente / fração
            <select
              required
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
        )}
        <div className="form-actions">
          <button type="button" className="button secondary" onClick={onClose}>
            Cancelar
          </button>
          <button className="button primary" disabled={form.processing}>
            <Save size={16} />
            {form.processing ? 'A guardar...' : 'Guardar etiqueta'}
          </button>
        </div>
      </form>
    </Modal>
  )
}
