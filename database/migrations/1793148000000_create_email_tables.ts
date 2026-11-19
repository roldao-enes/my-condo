import { BaseSchema } from '@adonisjs/lucid/schema'

export default class extends BaseSchema {
  async up() {
    this.schema.createTable('announcements', (table) => {
      table.increments('id')
      table.string('subject', 200).notNullable()
      table.text('body').notNullable()
      table.enum('status', ['draft', 'queued']).notNullable().defaultTo('draft')
      table
        .integer('created_by')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('users')
        .onDelete('RESTRICT')
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.timestamp('queued_at', { useTz: true }).nullable()
    })
    this.schema.createTable('email_jobs', (table) => {
      table.increments('id')
      table
        .integer('announcement_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('announcements')
        .onDelete('RESTRICT')
      table
        .integer('resident_id')
        .unsigned()
        .nullable()
        .references('id')
        .inTable('residents')
        .onDelete('SET NULL')
      table.string('recipient_name', 160).notNullable()
      table.string('recipient_email', 254).notNullable()
      table.text('unit_labels').notNullable().defaultTo('')
      table
        .enum('status', ['pending', 'sending', 'sent', 'failed'])
        .notNullable()
        .defaultTo('pending')
      table.integer('attempts').notNullable().defaultTo(0)
      table.timestamp('next_attempt_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.text('last_error').nullable()
      table.timestamp('sent_at', { useTz: true }).nullable()
      table.timestamp('created_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.unique(['announcement_id', 'recipient_email'])
      table.check('attempts >= 0')
      table.index(['announcement_id'])
    })
    this.schema.raw(
      "CREATE INDEX email_jobs_due ON email_jobs (next_attempt_at) WHERE status IN ('pending', 'sending')"
    )
    this.schema.createTable('email_attempts', (table) => {
      table.increments('id')
      table
        .integer('email_job_id')
        .unsigned()
        .notNullable()
        .references('id')
        .inTable('email_jobs')
        .onDelete('RESTRICT')
      table.timestamp('attempted_at', { useTz: true }).notNullable().defaultTo(this.now())
      table.enum('outcome', ['ok', 'error']).notNullable()
      table.text('error').nullable()
      table.index(['email_job_id'])
    })
  }

  async down() {
    this.schema.dropTable('email_attempts')
    this.schema.dropTable('email_jobs')
    this.schema.dropTable('announcements')
  }
}
