import { BaseSchema } from '@adonisjs/lucid/schema'

export default class extends BaseSchema {
  async up() {
    this.schema.createTable('finance_transactions', (table) => {
      table.increments('id')
      table.string('fingerprint', 64).notNullable().unique()
      table.date('date').notNullable()
      table.date('value_date').notNullable()
      table.string('description', 500).notNullable()
      table.integer('amount').notNullable()
      table.integer('balance').notNullable()
      table
        .enum('category', ['unclassified', 'recurring', 'extraordinary'])
        .notNullable()
        .defaultTo('unclassified')
      table
        .integer('imported_by')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('users')
        .onDelete('RESTRICT')
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.check('amount <> 0')
      table.index(['date'])
    })
    this.schema.createTable('finance_charges', (table) => {
      table.increments('id')
      table
        .integer('resident_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('residents')
        .onDelete('RESTRICT')
      table
        .integer('unit_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('units')
        .onDelete('RESTRICT')
      table.string('description', 200).notNullable()
      table.date('due_date').notNullable()
      table.integer('amount').notNullable()
      table
        .integer('created_by')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('users')
        .onDelete('RESTRICT')
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.check('amount > 0')
      table.index(['resident_id', 'unit_id'])
    })
    this.schema.createTable('finance_payments', (table) => {
      table.increments('id')
      table
        .integer('transaction_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('finance_transactions')
        .onDelete('RESTRICT')
      table
        .integer('charge_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('finance_charges')
        .onDelete('RESTRICT')
      table.integer('amount').notNullable()
      table.unique(['transaction_id', 'charge_id'])
      table.check('amount > 0')
    })
  }

  async down() {
    this.schema.dropTable('finance_payments')
    this.schema.dropTable('finance_charges')
    this.schema.dropTable('finance_transactions')
  }
}
