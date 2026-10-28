import { BaseSchema } from '@adonisjs/lucid/schema'
import { normalizeName } from '../../app/finances/domain/tags.js'

export default class extends BaseSchema {
  async up() {
    this.schema.createTable('finance_tags', (table) => {
      table.increments('id')
      table.string('name', 160).notNullable()
      table.string('pattern', 500).notNullable()
      table.enum('match_type', ['exact', 'contains']).notNullable()
      table.enum('direction', ['debit', 'credit']).notNullable()
      table.enum('category', ['recurring', 'extraordinary', 'resident_payment']).notNullable()
      table
        .integer('resident_id')
        .unsigned()
        .nullable()
        .references('id')
        .inTable('residents')
        .onDelete('RESTRICT')
      table
        .integer('unit_id')
        .unsigned()
        .nullable()
        .references('id')
        .inTable('units')
        .onDelete('RESTRICT')
      table
        .integer('created_by')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('users')
        .onDelete('RESTRICT')
      table.unique(['pattern', 'match_type', 'direction'])
      table.check(
        "(category = 'resident_payment' AND direction = 'credit' AND resident_id IS NOT NULL AND unit_id IS NOT NULL) OR (category IN ('recurring', 'extraordinary') AND direction = 'debit' AND resident_id IS NULL AND unit_id IS NULL)"
      )
    })
    this.schema.alterTable('finance_transactions', (table) => {
      table
        .integer('tag_id')
        .unsigned()
        .nullable()
        .references('id')
        .inTable('finance_tags')
        .onDelete('SET NULL')
      table
        .integer('resident_id')
        .unsigned()
        .nullable()
        .references('id')
        .inTable('residents')
        .onDelete('RESTRICT')
      table
        .integer('unit_id')
        .unsigned()
        .nullable()
        .references('id')
        .inTable('units')
        .onDelete('RESTRICT')
      table.check(
        '(resident_id IS NULL AND unit_id IS NULL) OR (amount > 0 AND resident_id IS NOT NULL AND unit_id IS NOT NULL)',
        [],
        'finance_transactions_resident_check'
      )
    })
    this.defer(async (db) => {
      const expenses = await db
        .from('finance_transactions')
        .where('amount', '<', 0)
        .whereIn('category', ['recurring', 'extraordinary'])
        .select('id', 'description', 'category', 'imported_by')
      const payments = await db
        .from('finance_payments')
        .join('finance_transactions', 'finance_transactions.id', 'finance_payments.transaction_id')
        .join('finance_charges', 'finance_charges.id', 'finance_payments.charge_id')
        .select(
          'finance_transactions.id',
          'finance_transactions.description',
          'imported_by',
          'finance_charges.resident_id',
          'finance_charges.unit_id'
        )
      const candidates = [
        ...expenses.map((row) => ({ ...row, resident_id: null, unit_id: null })),
        ...payments.map((row) => ({ ...row, category: 'resident_payment' })),
      ]
      const groups = new Map<string, typeof candidates>()
      for (const candidate of candidates) {
        const key = `${candidate.category === 'resident_payment' ? 'credit' : 'debit'}:${normalizeName(candidate.description)}`
        groups.set(key, [...(groups.get(key) || []), candidate])
      }
      for (const group of groups.values()) {
        const first = group[0]
        const pattern = normalizeName(first.description)
        if (
          !pattern ||
          group.some(
            (row) =>
              row.category !== first.category ||
              row.resident_id !== first.resident_id ||
              row.unit_id !== first.unit_id
          )
        )
          continue
        const [tag] = await db
          .table('finance_tags')
          .insert({
            name: first.description.slice(0, 160),
            pattern,
            match_type: 'exact',
            direction: first.category === 'resident_payment' ? 'credit' : 'debit',
            category: first.category,
            resident_id: first.resident_id,
            unit_id: first.unit_id,
            created_by: first.imported_by,
          })
          .returning('id')
        await db
          .from('finance_transactions')
          .whereIn(
            'id',
            group.map((row) => row.id)
          )
          .update({ tag_id: tag.id, resident_id: first.resident_id, unit_id: first.unit_id })
      }
    })
  }

  async down() {
    this.schema.alterTable('finance_transactions', (table) => {
      table.dropChecks('finance_transactions_resident_check')
      table.dropColumns('tag_id', 'resident_id', 'unit_id')
    })
    this.schema.dropTable('finance_tags')
  }
}
