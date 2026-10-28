import { createHash } from 'node:crypto'
import { Exception } from '@adonisjs/core/exceptions'
import db from '@adonisjs/lucid/services/db'
import type { TransactionClientContract } from '@adonisjs/lucid/types/database'
import { parseStatement } from './domain/statement.js'
import { matchTag, normalizeName, type FinanceTag } from './domain/tags.js'

type TagInput = {
  name: string
  pattern: string
  matchType: 'exact' | 'contains'
  category: FinanceTag['category']
  residentId?: number | null
  unitId?: number | null
}

export default class FinanceService {
  async importStatement(actorId: number, content: string) {
    const rows = parseStatement(content)
    const datedPair = rows.findIndex((row, index) => index > 0 && row.date !== rows[index - 1].date)
    const newestFirst =
      datedPair > 0
        ? rows[datedPair].date < rows[datedPair - 1].date
        : rows.length > 1 && rows[1].balance + rows[0].amount === rows[0].balance
    if (newestFirst) rows.reverse()
    return db.transaction(async (trx) => {
      const tags: FinanceTag[] = await trx.from('finance_tags')
      const occurrences = new Map<string, number>()
      const records = rows.map((row) => {
        const key = JSON.stringify(row)
        const occurrence = (occurrences.get(key) || 0) + 1
        occurrences.set(key, occurrence)
        const tag = matchTag(row.description, row.amount, tags)
        return {
          fingerprint: createHash('sha256').update(`${key}:${occurrence}`).digest('hex'),
          date: row.date,
          value_date: row.valueDate,
          description: row.description,
          amount: row.amount,
          balance: row.balance,
          imported_by: actorId,
          category: tag && tag.category !== 'resident_payment' ? tag.category : 'unclassified',
          tag_id: tag?.id || null,
          resident_id: tag?.resident_id || null,
          unit_id: tag?.unit_id || null,
        }
      })
      let imported = 0
      for (let offset = 0; offset < records.length; offset += 500) {
        const inserted = await trx
          .knexQuery()
          .table('finance_transactions')
          .insert(records.slice(offset, offset + 500))
          .onConflict('fingerprint')
          .ignore()
          .returning('id')
        imported += inserted.length
      }
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.imported',
        entity_type: 'statement',
        entity_id: 0,
        metadata: JSON.stringify({ imported, skipped: rows.length - imported }),
      })
      return { imported, skipped: rows.length - imported }
    })
  }

  async list() {
    const transactions = await db
      .from('finance_transactions')
      .leftJoin('finance_tags', 'finance_tags.id', 'finance_transactions.tag_id')
      .leftJoin('residents', 'residents.id', 'finance_transactions.resident_id')
      .leftJoin('units', 'units.id', 'finance_transactions.unit_id')
      .select(
        'finance_transactions.id',
        'description',
        'amount',
        'balance',
        'finance_transactions.category',
        'tag_id',
        'finance_tags.name as tag_name',
        'finance_transactions.resident_id',
        'finance_transactions.unit_id',
        'residents.name as resident_name',
        'units.label as unit_label'
      )
      .select(db.raw("to_char(date, 'YYYY-MM-DD') as date"))
      .select(db.raw("to_char(value_date, 'YYYY-MM-DD') as value_date"))
      .orderBy('date', 'desc')
      .orderBy('finance_transactions.id', 'desc')
    const charges = await db
      .from('finance_charges')
      .join('residents', 'residents.id', 'finance_charges.resident_id')
      .join('units', 'units.id', 'finance_charges.unit_id')
      .select(
        'finance_charges.id',
        'resident_id',
        'unit_id',
        'description',
        'amount',
        'residents.name',
        'units.label as unit_label'
      )
      .select(db.raw("to_char(due_date, 'YYYY-MM-DD') as due_date"))
      .orderBy('due_date')
      .orderBy('finance_charges.id')
    const payments = await db
      .from('finance_payments')
      .select('id', 'transaction_id', 'charge_id', 'amount')
    const accounts = await db
      .from('occupancies')
      .join('residents', 'residents.id', 'occupancies.resident_id')
      .join('units', 'units.id', 'occupancies.unit_id')
      .distinct('resident_id', 'unit_id', 'residents.name', 'units.label as unit_label')
      .orderBy('residents.name')
    const tags = await db
      .from('finance_tags')
      .leftJoin('residents', 'residents.id', 'finance_tags.resident_id')
      .leftJoin('units', 'units.id', 'finance_tags.unit_id')
      .select('finance_tags.*', 'residents.name as resident_name', 'units.label as unit_label')
      .orderBy('finance_tags.name')
    return { transactions, charges, payments, accounts, tags }
  }

  async classify(actorId: number, id: number, category: string, remember = true) {
    return db.transaction(async (trx) => {
      const transaction = await trx
        .from('finance_transactions')
        .where('id', id)
        .where('amount', '<', 0)
        .forUpdate()
        .first()
      if (!transaction) throw new Exception('Despesa nao encontrada.', { status: 404 })
      const tagId =
        remember && (category === 'recurring' || category === 'extraordinary')
          ? await this.writeTag(trx, actorId, {
              name: transaction.description.slice(0, 160),
              pattern: transaction.description,
              matchType: 'exact',
              category,
            })
          : null
      const updated = await trx
        .from('finance_transactions')
        .where('id', id)
        .where('amount', '<', 0)
        .update({ category, tag_id: tagId })
        .returning('id')
      if (!updated.length) throw new Exception('Despesa nao encontrada.', { status: 404 })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.classified',
        entity_type: 'transaction',
        entity_id: id,
        metadata: JSON.stringify({ category, tagId }),
      })
    })
  }

  async createCharge(
    actorId: number,
    input: {
      residentId: number
      unitId: number
      description: string
      dueDate: string
      amount: number
    }
  ) {
    return db.transaction(async (trx) => {
      const relationship = await trx
        .from('occupancies')
        .where('resident_id', input.residentId)
        .where('unit_id', input.unitId)
        .first()
      if (!relationship)
        throw new Exception('O residente nao tem relacao com esta fracao.', { status: 422 })
      const [charge] = await trx
        .table('finance_charges')
        .insert({
          resident_id: input.residentId,
          unit_id: input.unitId,
          description: input.description,
          due_date: input.dueDate,
          amount: input.amount,
          created_by: actorId,
        })
        .returning('id')
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.charged',
        entity_type: 'charge',
        entity_id: charge.id,
        metadata: JSON.stringify(input),
      })
      return charge.id as number
    })
  }

  async allocate(
    actorId: number,
    transactionId: number,
    chargeId: number,
    amount: number,
    remember = true
  ) {
    return db.transaction(async (trx) => {
      const transaction = await trx
        .from('finance_transactions')
        .where('id', transactionId)
        .forUpdate()
        .first()
      const charge = await trx.from('finance_charges').where('id', chargeId).forUpdate().first()
      if (!transaction || !charge)
        throw new Exception('Movimento ou quota nao encontrado.', { status: 404 })
      const transactionTotal = await trx
        .from('finance_payments')
        .where('transaction_id', transactionId)
        .sum('amount as total')
        .first()
      const chargeTotal = await trx
        .from('finance_payments')
        .where('charge_id', chargeId)
        .sum('amount as total')
        .first()
      if (
        !Number.isSafeInteger(amount) ||
        amount <= 0 ||
        transaction.amount <= 0 ||
        amount > transaction.amount - Number(transactionTotal.total || 0) ||
        amount > charge.amount - Number(chargeTotal.total || 0)
      ) {
        throw new Exception('O pagamento excede o valor disponivel ou o saldo da quota.', {
          status: 422,
        })
      }
      if (remember) {
        const other = await trx
          .from('finance_payments')
          .join('finance_charges', 'finance_charges.id', 'finance_payments.charge_id')
          .where('transaction_id', transactionId)
          .where((query) =>
            query.whereNot('resident_id', charge.resident_id).orWhereNot('unit_id', charge.unit_id)
          )
          .first()
        if (other)
          throw new Exception('Pagamento partilhado: desative a opcao de memorizar o nome.', {
            status: 422,
          })
        const tagId = await this.writeTag(trx, actorId, {
          name: transaction.description.slice(0, 160),
          pattern: transaction.description,
          matchType: 'exact',
          category: 'resident_payment',
          residentId: charge.resident_id,
          unitId: charge.unit_id,
        })
        await trx
          .from('finance_transactions')
          .where('id', transactionId)
          .update({ tag_id: tagId, resident_id: charge.resident_id, unit_id: charge.unit_id })
      }
      await trx
        .knexQuery()
        .table('finance_payments')
        .insert({ transaction_id: transactionId, charge_id: chargeId, amount })
        .onConflict(['transaction_id', 'charge_id'])
        .merge({ amount: trx.knexClient.raw('finance_payments.amount + ?', [amount]) })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.payment_matched',
        entity_type: 'transaction',
        entity_id: transactionId,
        metadata: JSON.stringify({ chargeId, amount }),
      })
    })
  }

  async unallocate(actorId: number, id: number) {
    return db.transaction(async (trx) => {
      const payment = await trx.from('finance_payments').where('id', id).first()
      if (!payment) throw new Exception('Pagamento nao encontrado.', { status: 404 })
      await trx.from('finance_transactions').where('id', payment.transaction_id).forUpdate().first()
      await trx.from('finance_charges').where('id', payment.charge_id).forUpdate().first()
      const deleted = await trx.from('finance_payments').where('id', id).delete().returning('id')
      if (!deleted.length) throw new Exception('Pagamento nao encontrado.', { status: 404 })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.payment_unmatched',
        entity_type: 'transaction',
        entity_id: payment.transaction_id,
        metadata: JSON.stringify(payment),
      })
    })
  }

  async identify(
    actorId: number,
    transactionId: number,
    residentId: number | null,
    unitId: number | null,
    remember = true
  ) {
    return db.transaction(async (trx) => {
      const transaction = await trx
        .from('finance_transactions')
        .where('id', transactionId)
        .where('amount', '>', 0)
        .forUpdate()
        .first()
      if (!transaction) throw new Exception('Entrada nao encontrada.', { status: 404 })
      if (residentId !== null || unitId !== null)
        await this.validateAccount(trx, residentId, unitId)
      const tagId =
        residentId && unitId && remember
          ? await this.writeTag(trx, actorId, {
              name: transaction.description.slice(0, 160),
              pattern: transaction.description,
              matchType: 'exact',
              category: 'resident_payment',
              residentId,
              unitId,
            })
          : null
      await trx
        .from('finance_transactions')
        .where('id', transactionId)
        .update({ resident_id: residentId, unit_id: unitId, tag_id: tagId })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.resident_identified',
        entity_type: 'transaction',
        entity_id: transactionId,
        metadata: JSON.stringify({ residentId, unitId, tagId }),
      })
    })
  }

  async saveTag(actorId: number, input: TagInput, id?: number) {
    return db.transaction(async (trx) => this.writeTag(trx, actorId, input, id))
  }

  private async validateAccount(
    trx: TransactionClientContract,
    residentId: number | null | undefined,
    unitId: number | null | undefined
  ) {
    if (
      !residentId ||
      !unitId ||
      !(await trx
        .from('occupancies')
        .where('resident_id', residentId)
        .where('unit_id', unitId)
        .first())
    ) {
      throw new Exception('O residente nao tem relacao com esta fracao.', { status: 422 })
    }
  }

  private async writeTag(
    trx: TransactionClientContract,
    actorId: number,
    input: TagInput,
    id?: number
  ) {
    const pattern = normalizeName(input.pattern)
    if (pattern.length < 3 || pattern.length > 500)
      throw new Exception('Indique um nome com pelo menos 3 caracteres.', { status: 422 })
    if (input.category === 'resident_payment')
      await this.validateAccount(trx, input.residentId, input.unitId)
    const record = {
      name: input.name.trim(),
      pattern,
      match_type: input.matchType,
      direction: input.category === 'resident_payment' ? 'credit' : 'debit',
      category: input.category,
      resident_id: input.category === 'resident_payment' ? input.residentId : null,
      unit_id: input.category === 'resident_payment' ? input.unitId : null,
    }
    let tagId: number
    if (id !== undefined) {
      const updated = await trx.from('finance_tags').where('id', id).update(record).returning('id')
      if (!updated.length) throw new Exception('Etiqueta nao encontrada.', { status: 404 })
      tagId = updated[0].id
    } else {
      const [tag] = await trx
        .knexQuery()
        .table('finance_tags')
        .insert({ ...record, created_by: actorId })
        .onConflict(['pattern', 'match_type', 'direction'])
        .merge(record)
        .returning('id')
      tagId = tag.id
    }
    await trx.table('audit_events').insert({
      actor_id: actorId,
      action: 'finance.tag_saved',
      entity_type: 'finance_tag',
      entity_id: tagId,
      metadata: JSON.stringify(record),
    })
    return tagId
  }

  async deleteTag(actorId: number, id: number) {
    return db.transaction(async (trx) => {
      const [tag] = await trx.from('finance_tags').where('id', id).delete().returning('*')
      if (!tag) throw new Exception('Etiqueta nao encontrada.', { status: 404 })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'finance.tag_deleted',
        entity_type: 'finance_tag',
        entity_id: id,
        metadata: JSON.stringify(tag),
      })
    })
  }
}
