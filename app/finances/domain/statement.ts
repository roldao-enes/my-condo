import { parse } from 'csv-parse/sync'
import { DateTime } from 'luxon'

export type StatementRow = {
  date: string
  valueDate: string
  description: string
  amount: number
  balance: number
}

export function parseMoney(value: string): number {
  const normalized = value.trim().replace(/\s|€/g, '')
  const decimal = normalized.includes(',')
    ? normalized.replace(/\./g, '').replace(',', '.')
    : normalized
  if (!/^-?\d+(\.\d{1,2})?$/.test(decimal)) throw new Error('Montante invalido.')
  const cents = Math.round(Number(decimal) * 100)
  if (!Number.isSafeInteger(cents) || Math.abs(cents) > 2_000_000_000) {
    throw new Error('Montante fora do limite.')
  }
  return cents
}

export function parseStatement(content: string): StatementRow[] {
  let rows: string[][] = []
  for (const delimiter of ['\t', ';', ',']) {
    try {
      const candidate: string[][] = parse(content, {
        bom: true,
        delimiter,
        skip_empty_lines: true,
        trim: true,
      })
      if (candidate.length && candidate.every((row) => row.length === 5)) {
        rows = candidate
        break
      }
    } catch {
      continue
    }
  }
  if (!rows.length || rows.length > 10000) throw new Error('Importe entre 1 e 10000 movimentos.')
  const date = (value: string) => {
    const parsed = /^\d{4}-\d{2}-\d{2}$/.test(value)
      ? DateTime.fromISO(value)
      : DateTime.fromFormat(value, 'dd/MM/yyyy')
    if (!parsed.isValid) throw new Error('Data invalida.')
    return parsed.toISODate()!
  }
  if (!/^\d{4}-\d{2}-\d{2}$|^\d{2}\/\d{2}\/\d{4}$/.test(rows[0][0])) {
    const header = rows.shift()!
    if (!/^(data|date|booking date)$/i.test(header[0])) throw new Error('Cabecalho invalido.')
  }
  if (!rows.length) throw new Error('O ficheiro nao contem movimentos.')
  return rows.map((row, index) => {
    try {
      if (row.length !== 5 || !row[2] || row[2].length > 500) {
        throw new Error('Esperadas 5 colunas: data, data valor, descricao, montante e saldo.')
      }
      const amount = parseMoney(row[3])
      if (!amount) throw new Error('O montante nao pode ser zero.')
      return {
        date: date(row[0]),
        valueDate: date(row[1]),
        description: row[2],
        amount,
        balance: parseMoney(row[4]),
      }
    } catch (error) {
      throw new Error(`Movimento ${index + 1}: ${(error as Error).message}`)
    }
  })
}
