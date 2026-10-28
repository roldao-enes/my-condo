import { Head, Link, useForm } from '@inertiajs/react'
import { useState } from 'react'
import {
  ArrowDownLeft,
  ArrowRight,
  ArrowUpRight,
  Building2,
  DoorOpen,
  History,
  Plus,
  Search,
  UserPlus,
  Users,
  LogOut,
  Mail,
  Phone,
} from 'lucide-react'
import Shell from '../../components/shell'
import Modal from '../../components/modal'
import {
  date,
  initials,
  roles,
  today,
  type Relationship,
  type Resident,
  type Shared,
  type Unit,
} from '../../lib/types'

interface Props extends Shared {
  units: Unit[]
  residents: Resident[]
  relationships: Relationship[]
  view: 'overview' | 'units' | 'residents'
}
type Dialog =
  | { kind: 'unit' }
  | { kind: 'onboard'; unitId?: number }
  | { kind: 'offboard'; relationship: Relationship }
  | { kind: 'detail'; unit: Unit }
  | null

export default function Residency({ units, residents, relationships, view }: Props) {
  const [query, setQuery] = useState('')
  const [history, setHistory] = useState(false)
  const [dialog, setDialog] = useState<Dialog>(null)
  const active = relationships.filter((relationship) => !relationship.end_date)
  const occupied = new Set(active.map((relationship) => relationship.unit_id))
  const residentCount = new Set(active.map((relationship) => relationship.resident_id)).size
  const close = () => setDialog(null)
  const titles = { overview: 'Tudo em casa.', units: 'Frações', residents: 'Residentes' }
  const filteredUnits = units.filter((unit) =>
    `${unit.label} ${unit.floor}`
      .toLocaleLowerCase('pt-PT')
      .includes(query.toLocaleLowerCase('pt-PT'))
  )
  const filteredRelationships = relationships.filter(
    (relationship) =>
      Boolean(relationship.end_date) === history &&
      `${relationship.name} ${relationship.unit_label} ${relationship.email || ''}`
        .toLocaleLowerCase('pt-PT')
        .includes(query.toLocaleLowerCase('pt-PT'))
  )

  return (
    <Shell view={view}>
      <Head title={view === 'overview' ? 'Visão geral' : titles[view]} />
      <div className="page-heading">
        <div>
          <div className="eyebrow">
            {view === 'overview' ? 'O MEU CONDOMÍNIO' : 'GESTÃO DO CONDOMÍNIO'}
          </div>
          <h1>{titles[view]}</h1>
          <p className="muted">
            {view === 'overview'
              ? 'O seu condomínio, num só lugar.'
              : view === 'units'
                ? `${units.length} frações no condomínio`
                : `${residentCount} residentes com relação ativa`}
          </p>
        </div>
        <div className="heading-actions">
          {view !== 'residents' && (
            <button
              className={`button ${view === 'units' ? 'primary' : 'secondary'}`}
              onClick={() => setDialog({ kind: 'unit' })}
            >
              <Plus size={17} />
              Nova fração
            </button>
          )}
          {view !== 'units' && (
            <button
              className="button primary"
              disabled={!units.length}
              title={!units.length ? 'Crie uma fração primeiro' : undefined}
              onClick={() => setDialog({ kind: 'onboard' })}
            >
              <UserPlus size={17} />
              Adicionar residente
            </button>
          )}
        </div>
      </div>

      {view === 'overview' && (
        <>
          <div className="stats-grid">
            <Stat
              label="Frações"
              value={units.length}
              caption={`${units.length - occupied.size} sem residentes ativos`}
              icon={Building2}
              tone="green"
            />
            <Stat
              label="Residentes ativos"
              value={residentCount}
              caption={`${active.filter((relationship) => relationship.role === 'owner').length} relações de propriedade`}
              icon={Users}
              tone="blue"
            />
            <Stat
              label="Frações ocupadas"
              value={`${occupied.size}/${units.length}`}
              caption="Com pelo menos uma relação ativa"
              icon={DoorOpen}
              tone="rose"
            />
          </div>
          <div className="section-heading">
            <h2>
              As suas frações <span className="count">{units.length}</span>
            </h2>
            <Link href="/units" className="text-link">
              Ver todas
              <ArrowRight size={16} />
            </Link>
          </div>
          {units.length ? (
            <div className="unit-grid">
              {units.slice(0, 6).map((unit) => {
                const people = active.filter((relationship) => relationship.unit_id === unit.id)
                return (
                  <button
                    key={unit.id}
                    className="unit-card"
                    onClick={() => setDialog({ kind: 'detail', unit })}
                  >
                    <div className="unit-card-top">
                      <span className="unit-icon">
                        <DoorOpen size={22} />
                      </span>
                      <ArrowUpRight size={17} />
                    </div>
                    <h3>{unit.label}</h3>
                    <span className="muted">Piso {unit.floor}</span>
                    <div className="unit-card-bottom">
                      <div className="avatar-stack">
                        {people.slice(0, 3).map((person) => (
                          <span className="avatar small" key={person.id}>
                            {initials(person.name)}
                          </span>
                        ))}
                      </div>
                      <span className={`badge ${people.length ? 'green' : 'neutral'}`}>
                        {people.length
                          ? `${people.length} ${people.length === 1 ? 'residente' : 'residentes'}`
                          : 'Sem residentes'}
                      </span>
                    </div>
                  </button>
                )
              })}
            </div>
          ) : (
            <Empty
              icon={Building2}
              title="O seu condomínio começa aqui."
              text="Ainda não existem frações registadas."
              action="Criar primeira fração"
              onClick={() => setDialog({ kind: 'unit' })}
            />
          )}
          <div className="section-heading activity-heading">
            <h2>Movimentos recentes</h2>
            <Link href="/residents" className="text-link">
              Ver residentes
              <ArrowRight size={16} />
            </Link>
          </div>
          {relationships.length ? (
            <div className="activity-list">
              {relationships.slice(0, 5).map((relationship) => (
                <div className="activity-row" key={relationship.id}>
                  <span className={`activity-icon ${relationship.end_date ? 'departed' : ''}`}>
                    {relationship.end_date ? (
                      <ArrowUpRight size={18} />
                    ) : (
                      <ArrowDownLeft size={18} />
                    )}
                  </span>
                  <div>
                    <strong>{relationship.name}</strong>
                    <span>
                      {relationship.end_date ? 'Saída registada' : 'Entrada registada'} ·{' '}
                      {relationship.unit_label}
                    </span>
                  </div>
                  <time>{date(relationship.end_date || relationship.start_date)}</time>
                </div>
              ))}
            </div>
          ) : (
            <div className="quiet-empty">
              <History size={20} />
              Ainda não existem movimentos.
            </div>
          )}
        </>
      )}

      {view === 'units' && (
        <>
          <div className="list-toolbar">
            <SearchBox
              value={query}
              onChange={setQuery}
              placeholder="Pesquisar fração ou piso..."
            />
            <span className="muted small-text">{filteredUnits.length} resultados</span>
          </div>
          {filteredUnits.length ? (
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Fração</th>
                    <th>Piso</th>
                    <th>Residentes ativos</th>
                    <th>Estado</th>
                    <th>
                      <span className="sr-only">Acoes</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {filteredUnits.map((unit) => {
                    const people = active.filter((relationship) => relationship.unit_id === unit.id)
                    return (
                      <tr key={unit.id}>
                        <td>
                          <button
                            className="cell-link"
                            onClick={() => setDialog({ kind: 'detail', unit })}
                          >
                            <span className="table-unit-icon">
                              <DoorOpen size={18} />
                            </span>
                            {unit.label}
                          </button>
                        </td>
                        <td>{unit.floor}</td>
                        <td>
                          {people.length ? (
                            people.map((person) => person.name).join(', ')
                          ) : (
                            <span className="muted">Nenhum</span>
                          )}
                        </td>
                        <td>
                          <span className={`badge ${people.length ? 'green' : 'neutral'}`}>
                            {people.length ? 'Ocupada' : 'Sem residentes'}
                          </span>
                        </td>
                        <td>
                          <button
                            className="icon-button"
                            title={`Abrir ${unit.label}`}
                            aria-label={`Abrir ${unit.label}`}
                            onClick={() => setDialog({ kind: 'detail', unit })}
                          >
                            <ArrowUpRight size={18} />
                          </button>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          ) : (
            <Empty
              icon={DoorOpen}
              title={query ? 'Nenhuma fração encontrada.' : 'Ainda não existem frações.'}
              text={
                query ? 'Experimente outra pesquisa.' : 'Adicione a primeira fração do condomínio.'
              }
            />
          )}
        </>
      )}

      {view === 'residents' && (
        <>
          <div className="list-toolbar resident-toolbar">
            <div className="segments" aria-label="Estado dos residentes">
              <button
                aria-pressed={!history}
                className={!history ? 'selected' : ''}
                onClick={() => setHistory(false)}
              >
                Ativos<span>{active.length}</span>
              </button>
              <button
                aria-pressed={history}
                className={history ? 'selected' : ''}
                onClick={() => setHistory(true)}
              >
                <History size={15} />
                Histórico<span>{relationships.length - active.length}</span>
              </button>
            </div>
            <SearchBox
              value={query}
              onChange={setQuery}
              placeholder="Pesquisar nome, email ou fração..."
            />
          </div>
          {filteredRelationships.length ? (
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Residente</th>
                    <th>Fração</th>
                    <th>Relação</th>
                    <th>{history ? 'Período' : 'Entrada'}</th>
                    <th>
                      <span className="sr-only">Acoes</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {filteredRelationships.map((relationship) => (
                    <tr key={relationship.id}>
                      <td>
                        <div className="person-cell">
                          <span className="avatar">{initials(relationship.name)}</span>
                          <div>
                            <strong>{relationship.name}</strong>
                            <small>
                              {relationship.email || relationship.phone || 'Sem contacto'}
                            </small>
                          </div>
                        </div>
                      </td>
                      <td>
                        <span className="unit-tag">{relationship.unit_label}</span>
                      </td>
                      <td>{roles[relationship.role]}</td>
                      <td className="date-cell">
                        {date(relationship.start_date)}
                        {relationship.end_date && (
                          <>
                            <br />
                            <span className="muted">a {date(relationship.end_date)}</span>
                          </>
                        )}
                      </td>
                      <td>
                        {!relationship.end_date && (
                          <button
                            className="icon-button"
                            title={`Registar saída de ${relationship.name}`}
                            aria-label={`Registar saída de ${relationship.name}`}
                            onClick={() => setDialog({ kind: 'offboard', relationship })}
                          >
                            <LogOut size={18} />
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <Empty
              icon={history ? History : Users}
              title={
                query
                  ? 'Nenhum residente encontrado.'
                  : history
                    ? 'Sem saídas registadas.'
                    : 'Ainda não existem residentes ativos.'
              }
              text={
                query
                  ? 'Experimente outra pesquisa.'
                  : history
                    ? 'As relações encerradas ficam guardadas aqui.'
                    : 'Associe um residente a uma fração.'
              }
            />
          )}
        </>
      )}

      {dialog?.kind === 'unit' && <UnitForm onClose={close} />}
      {dialog?.kind === 'onboard' && (
        <OnboardForm units={units} residents={residents} unitId={dialog.unitId} onClose={close} />
      )}
      {dialog?.kind === 'offboard' && (
        <OffboardForm relationship={dialog.relationship} onClose={close} />
      )}
      {dialog?.kind === 'detail' && (
        <Modal title={`Fração ${dialog.unit.label}`} onClose={close}>
          <div className="detail-summary">
            <DoorOpen size={24} />
            <span>Piso {dialog.unit.floor}</span>
          </div>
          <h3>Residentes e histórico</h3>
          <div className="detail-people">
            {relationships
              .filter((relationship) => relationship.unit_id === dialog.unit.id)
              .map((relationship) => (
                <div className="detail-person" key={relationship.id}>
                  <div className="person-cell">
                    <span className="avatar">{initials(relationship.name)}</span>
                    <div>
                      <strong>{relationship.name}</strong>
                      <small>
                        {roles[relationship.role]} · {date(relationship.start_date)}
                        {relationship.end_date ? ` a ${date(relationship.end_date)}` : ''}
                      </small>
                      {relationship.email && (
                        <small>
                          <Mail size={12} />
                          {relationship.email}
                        </small>
                      )}
                      {relationship.phone && (
                        <small>
                          <Phone size={12} />
                          {relationship.phone}
                        </small>
                      )}
                    </div>
                  </div>
                  <span className={`badge ${relationship.end_date ? 'neutral' : 'green'}`}>
                    {relationship.end_date ? 'Histórico' : 'Ativo'}
                  </span>
                </div>
              ))}
            {!relationships.some((relationship) => relationship.unit_id === dialog.unit.id) && (
              <p className="muted">Sem residentes registados.</p>
            )}
          </div>
          <div className="form-actions">
            <button
              className="button primary"
              onClick={() => setDialog({ kind: 'onboard', unitId: dialog.unit.id })}
            >
              <UserPlus size={17} />
              Adicionar residente
            </button>
          </div>
        </Modal>
      )}
    </Shell>
  )
}

function Stat({
  label,
  value,
  caption,
  icon: Icon,
  tone,
}: {
  label: string
  value: string | number
  caption: string
  icon: typeof Users
  tone: string
}) {
  return (
    <div className="stat">
      <div className="stat-top">
        <span>{label}</span>
        <span className={`stat-icon ${tone}`}>
          <Icon size={19} />
        </span>
      </div>
      <strong>{value}</strong>
      <small>{caption}</small>
    </div>
  )
}
function SearchBox({
  value,
  onChange,
  placeholder,
}: {
  value: string
  onChange: (value: string) => void
  placeholder: string
}) {
  return (
    <div className="search-box">
      <Search size={17} />
      <input
        type="search"
        aria-label={placeholder}
        placeholder={placeholder}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  )
}
function Empty({
  icon: Icon,
  title,
  text,
  action,
  onClick,
}: {
  icon: typeof Users
  title: string
  text: string
  action?: string
  onClick?: () => void
}) {
  return (
    <div className="empty-state">
      <span className="empty-icon">
        <Icon size={26} />
      </span>
      <h3>{title}</h3>
      <p className="muted">{text}</p>
      {action && (
        <button className="button primary" onClick={onClick}>
          <Plus size={17} />
          {action}
        </button>
      )}
    </div>
  )
}
function useComplete(onClose: () => void) {
  return (page: { props: Record<string, unknown> }) => {
    if (
      !Object.keys((page.props.errors || {}) as object).length &&
      !(page.props.flash as Shared['flash'])?.error
    )
      onClose()
  }
}
function UnitForm({ onClose }: { onClose: () => void }) {
  const form = useForm({ label: '', floor: '' })
  const complete = useComplete(onClose)
  return (
    <Modal title="Nova fração" onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          form.post('/units', { onSuccess: complete })
        }}
      >
        <label>
          Designação
          <input
            autoFocus
            required
            maxLength={80}
            placeholder="Ex.: 1.o Esq."
            value={form.data.label}
            onChange={(event) => form.setData('label', event.target.value)}
          />
        </label>
        <label>
          Piso
          <input
            required
            maxLength={40}
            placeholder="Ex.: 1"
            value={form.data.floor}
            onChange={(event) => form.setData('floor', event.target.value)}
          />
        </label>
        <FormActions onClose={onClose} processing={form.processing} label="Criar fração" />
      </form>
    </Modal>
  )
}
function OnboardForm({
  units,
  residents,
  unitId,
  onClose,
}: {
  units: Unit[]
  residents: Resident[]
  unitId?: number
  onClose: () => void
}) {
  const [existing, setExisting] = useState(false)
  const form = useForm({
    unitId: String(unitId || ''),
    residentId: '',
    name: '',
    email: '',
    phone: '',
    role: 'owner',
    startDate: today(),
  })
  const complete = useComplete(onClose)
  return (
    <Modal title="Adicionar residente" onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          form.transform((data) => ({
            unitId: data.unitId,
            role: data.role,
            startDate: data.startDate,
            ...(existing
              ? { residentId: data.residentId }
              : {
                  name: data.name,
                  ...(data.email ? { email: data.email } : {}),
                  ...(data.phone ? { phone: data.phone } : {}),
                }),
          }))
          form.post('/residents', { onSuccess: complete })
        }}
      >
        <label>
          Fração
          <select
            aria-label="Fração"
            autoFocus
            required
            value={form.data.unitId}
            onChange={(event) => form.setData('unitId', event.target.value)}
          >
            <option value="">Selecionar fração</option>
            {units.map((unit) => (
              <option value={unit.id} key={unit.id}>
                {unit.label} · Piso {unit.floor}
              </option>
            ))}
          </select>
        </label>
        {residents.length > 0 && (
          <div className="segments form-segments">
            <button
              type="button"
              aria-pressed={!existing}
              className={!existing ? 'selected' : ''}
              onClick={() => setExisting(false)}
            >
              Nova pessoa
            </button>
            <button
              type="button"
              aria-pressed={existing}
              className={existing ? 'selected' : ''}
              onClick={() => setExisting(true)}
            >
              Pessoa existente
            </button>
          </div>
        )}
        {existing ? (
          <label>
            Residente
            <select
              required
              value={form.data.residentId}
              onChange={(event) => form.setData('residentId', event.target.value)}
            >
              <option value="">Selecionar residente</option>
              {residents.map((resident) => (
                <option value={resident.id} key={resident.id}>
                  {resident.name}
                  {resident.email ? ` · ${resident.email}` : ''}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <>
            <label>
              Nome completo
              <input
                required
                maxLength={160}
                value={form.data.name}
                onChange={(event) => form.setData('name', event.target.value)}
                autoComplete="name"
              />
            </label>
            <div className="form-grid">
              <label>
                Email <span className="optional">opcional</span>
                <input
                  type="email"
                  maxLength={254}
                  value={form.data.email}
                  onChange={(event) => form.setData('email', event.target.value)}
                  autoComplete="email"
                />
              </label>
              <label>
                Telefone <span className="optional">opcional</span>
                <input
                  type="tel"
                  maxLength={40}
                  value={form.data.phone}
                  onChange={(event) => form.setData('phone', event.target.value)}
                  autoComplete="tel"
                />
              </label>
            </div>
          </>
        )}
        <div className="form-grid">
          <label>
            Relação
            <select
              value={form.data.role}
              onChange={(event) => form.setData('role', event.target.value)}
            >
              {Object.entries(roles).map(([role, label]) => (
                <option value={role} key={role}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Data de entrada
            <input
              type="date"
              required
              max={today()}
              value={form.data.startDate}
              onChange={(event) => form.setData('startDate', event.target.value)}
            />
          </label>
        </div>
        <FormActions onClose={onClose} processing={form.processing} label="Adicionar residente" />
      </form>
    </Modal>
  )
}
function OffboardForm({
  relationship,
  onClose,
}: {
  relationship: Relationship
  onClose: () => void
}) {
  const form = useForm({ endDate: today() })
  const complete = useComplete(onClose)
  return (
    <Modal title="Registar saída" onClose={onClose}>
      <div className="offboard-summary">
        <span className="avatar">{initials(relationship.name)}</span>
        <div>
          <strong>{relationship.name}</strong>
          <p className="muted">
            {relationship.unit_label} · {roles[relationship.role]}
          </p>
        </div>
      </div>
      <p className="muted">
        A relação com esta fração será encerrada. Os dados e o histórico serão preservados.
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault()
          form.post(`/occupancies/${relationship.id}/offboard`, { onSuccess: complete })
        }}
      >
        <label>
          Data de saída
          <input
            autoFocus
            type="date"
            min={relationship.start_date}
            max={today()}
            required
            value={form.data.endDate}
            onChange={(event) => form.setData('endDate', event.target.value)}
          />
        </label>
        <FormActions onClose={onClose} processing={form.processing} label="Confirmar saída" />
      </form>
    </Modal>
  )
}
function FormActions({
  onClose,
  processing,
  label,
}: {
  onClose: () => void
  processing: boolean
  label: string
}) {
  return (
    <div className="form-actions">
      <button type="button" className="button secondary" onClick={onClose}>
        Cancelar
      </button>
      <button type="submit" className="button primary" disabled={processing}>
        {processing ? 'A guardar...' : label}
        <ArrowRight size={16} />
      </button>
    </div>
  )
}
