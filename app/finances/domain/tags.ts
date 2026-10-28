export interface FinanceTag {
  id: number
  name: string
  pattern: string
  match_type: 'exact' | 'contains'
  category: 'recurring' | 'extraordinary' | 'resident_payment'
  resident_id: number | null
  unit_id: number | null
}

export function normalizeName(value: string) {
  return value
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toUpperCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()
    .replace(/\s+/g, ' ')
}

export function matchTag(description: string, amount: number, tags: FinanceTag[]) {
  const name = normalizeName(description)
  const matches = tags
    .filter(
      (tag) =>
        (tag.category === 'resident_payment' ? amount > 0 : amount < 0) &&
        (tag.match_type === 'exact'
          ? name === tag.pattern
          : ` ${name} `.includes(` ${tag.pattern} `))
    )
    .sort(
      (first, second) =>
        Number(second.match_type === 'exact') - Number(first.match_type === 'exact') ||
        second.pattern.length - first.pattern.length
    )
  const [first, second] = matches
  if (
    first &&
    second &&
    first.match_type === second.match_type &&
    first.pattern.length === second.pattern.length
  )
    return null
  if (
    first?.match_type === 'contains' &&
    matches.some((tag) => !` ${first.pattern} `.includes(` ${tag.pattern} `))
  )
    return null
  return first || null
}
