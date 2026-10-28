import { BaseSchema } from '@adonisjs/lucid/schema'

export default class extends BaseSchema {
  async up() {
    this.schema.createTable('units', (table) => {
      table.increments('id')
      table.string('label', 80).notNullable().unique()
      table.string('floor', 40).notNullable()
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
    })
    this.schema.createTable('residents', (table) => {
      table.increments('id')
      table.string('name', 160).notNullable()
      table.string('email', 254).nullable()
      table.string('phone', 40).nullable()
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
    })
    this.schema.createTable('occupancies', (table) => {
      table.increments('id')
      table
        .integer('unit_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('units')
        .onDelete('RESTRICT')
      table
        .integer('resident_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('residents')
        .onDelete('RESTRICT')
      table.enum('role', ['owner', 'tenant', 'occupant']).notNullable()
      table.date('start_date').notNullable()
      table.date('end_date').nullable()
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.check('end_date IS NULL OR end_date >= start_date')
      table.index(['unit_id', 'end_date'])
      table.index(['resident_id'])
    })
    this.schema.raw(
      'CREATE UNIQUE INDEX occupancies_active_relationship ON occupancies (unit_id, resident_id, role) WHERE end_date IS NULL'
    )
    this.schema.createTable('audit_events', (table) => {
      table.increments('id')
      table
        .integer('actor_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('users')
        .onDelete('RESTRICT')
      table.string('action', 80).notNullable()
      table.string('entity_type', 40).notNullable()
      table.integer('entity_id').notNullable()
      table.jsonb('metadata').notNullable().defaultTo('{}')
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
    })
  }

  async down() {
    this.schema.dropTable('audit_events')
    this.schema.dropTable('occupancies')
    this.schema.dropTable('residents')
    this.schema.dropTable('units')
  }
}
