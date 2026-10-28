export interface Shared extends Record<string, unknown> {
  user: { name: string | null; email: string } | null
  errors: Record<string, string>
  flash: { success?: string; error?: string }
}
export interface Unit {
  id: number
  label: string
  floor: string
}
export interface Resident {
  id: number
  name: string
  email: string | null
  phone: string | null
}
export interface Relationship extends Resident {
  unit_id: number
  resident_id: number
  unit_label: string
  role: 'owner' | 'tenant' | 'occupant'
  start_date: string
  end_date: string | null
}
export const roles = { owner: 'Proprietário', tenant: 'Arrendatário', occupant: 'Ocupante' }
export const today = () =>
  new Intl.DateTimeFormat('sv-SE', { timeZone: 'Europe/Lisbon' }).format(new Date())
export const date = (value: string) =>
  new Intl.DateTimeFormat('pt-PT', { timeZone: 'UTC' }).format(new Date(`${value}T00:00:00Z`))
export const initials = (name: string) =>
  name
    .split(' ')
    .filter(Boolean)
    .map((part) => part[0])
    .slice(0, 2)
    .join('')
    .toUpperCase()
