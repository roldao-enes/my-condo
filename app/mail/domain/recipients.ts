export interface RecipientRow {
  residentId: number
  name: string
  email: string | null
  unitLabel: string
}

export interface Recipient {
  residentId: number
  name: string
  email: string
  unitLabels: string[]
}

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

/**
 * Collapses active occupancy rows into one recipient per distinct email address.
 * Residents without a usable email are skipped; units are merged per recipient.
 */
export function buildRecipients(rows: RecipientRow[]): Recipient[] {
  const byEmail = new Map<string, Recipient>()
  for (const row of rows) {
    const email = row.email?.trim().toLowerCase()
    if (!email || email.length > 254 || !EMAIL.test(email)) continue
    const existing = byEmail.get(email)
    if (!existing) {
      byEmail.set(email, {
        residentId: row.residentId,
        name: row.name,
        email,
        unitLabels: [row.unitLabel],
      })
    } else if (!existing.unitLabels.includes(row.unitLabel)) {
      existing.unitLabels.push(row.unitLabel)
    }
  }
  return [...byEmail.values()]
}
