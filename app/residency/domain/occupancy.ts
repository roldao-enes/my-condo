export type ResidentRole = 'owner' | 'tenant' | 'occupant'

export class OccupancyValidationError extends Error {
  name = 'OccupancyValidationError'
}

export function validateOccupancyDates(startDate: string, endDate?: string | null) {
  const isDate = (value: string) => {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false
    const parsed = new Date(`${value}T00:00:00Z`)
    return !Number.isNaN(parsed.valueOf()) && parsed.toISOString().slice(0, 10) === value
  }

  if (!isDate(startDate) || (endDate && !isDate(endDate))) {
    throw new OccupancyValidationError('Introduza uma data valida.')
  }
  if (endDate && endDate < startDate) {
    throw new OccupancyValidationError('A data de saida nao pode ser anterior a data de entrada.')
  }
}
