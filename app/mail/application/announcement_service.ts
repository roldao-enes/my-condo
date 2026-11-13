import { Exception } from '@adonisjs/core/exceptions'
import db from '@adonisjs/lucid/services/db'
import { buildRecipients } from '../domain/recipients.js'

export default class AnnouncementService {
  async createDraft(actorId: number, input: { subject: string; body: string }) {
    return db.transaction(async (trx) => {
      const [announcement] = await trx
        .table('announcements')
        .insert({
          subject: input.subject.trim(),
          body: input.body.trim(),
          created_by: actorId,
        })
        .returning('id')
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'announcement.created',
        entity_type: 'announcement',
        entity_id: announcement.id,
      })
      return announcement.id as number
    })
  }

  /** Snapshots the current recipients into email jobs and locks the announcement. */
  async queue(actorId: number, announcementId: number, options: { unitId?: number } = {}) {
    return db.transaction(async (trx) => {
      const announcement = await trx
        .from('announcements')
        .where('id', announcementId)
        .forUpdate()
        .first()
      if (!announcement) throw new Exception('Comunicado nao encontrado.', { status: 404 })
      if (announcement.status !== 'draft')
        throw new Exception('Este comunicado ja foi colocado em fila.', { status: 409 })

      if (options.unitId) {
        const unit = await trx.from('units').where('id', options.unitId).first()
        if (!unit) throw new Exception('Fracao nao encontrada.', { status: 404 })
      }

      const rows = await trx
        .from('occupancies')
        .join('residents', 'residents.id', 'occupancies.resident_id')
        .join('units', 'units.id', 'occupancies.unit_id')
        .whereNull('occupancies.end_date')
        .if(options.unitId, (query) => query.where('occupancies.unit_id', options.unitId!))
        .orderBy(['units.label', 'residents.name'])
        .select(
          'residents.id as residentId',
          'residents.name as name',
          'residents.email as email',
          'units.label as unitLabel'
        )
      const recipients = buildRecipients(rows)
      if (recipients.length === 0)
        throw new Exception('Nao existem destinatarios com email para este comunicado.', {
          status: 422,
        })

      await trx.table('email_jobs').insert(
        recipients.map((recipient) => ({
          announcement_id: announcementId,
          resident_id: recipient.residentId,
          recipient_name: recipient.name,
          recipient_email: recipient.email,
          unit_labels: recipient.unitLabels.join(', '),
        }))
      )
      await trx
        .from('announcements')
        .where('id', announcementId)
        .update({ status: 'queued', queued_at: new Date() })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'announcement.queued',
        entity_type: 'announcement',
        entity_id: announcementId,
        metadata: JSON.stringify({
          recipients: recipients.length,
          unitId: options.unitId ?? null,
        }),
      })
      return recipients.length
    })
  }

  async list() {
    return db
      .from('announcements')
      .leftJoin('email_jobs', 'email_jobs.announcement_id', 'announcements.id')
      .groupBy('announcements.id')
      .orderBy('announcements.id', 'desc')
      .select(
        'announcements.id',
        'announcements.subject',
        'announcements.status',
        'announcements.created_at as createdAt'
      )
      .select(db.raw('count(email_jobs.id)::int as total'))
      .select(db.raw("count(*) filter (where email_jobs.status = 'sent')::int as sent"))
      .select(db.raw("count(*) filter (where email_jobs.status = 'failed')::int as failed"))
  }

  async show(announcementId: number) {
    const announcement = await db
      .from('announcements')
      .where('id', announcementId)
      .select('id', 'subject', 'body', 'status', 'created_at as createdAt')
      .first()
    if (!announcement) throw new Exception('Comunicado nao encontrado.', { status: 404 })
    const units = await db.from('units').orderBy('label').select('id', 'label')
    const jobs = await db
      .from('email_jobs')
      .where('announcement_id', announcementId)
      .orderBy('recipient_name')
      .select(
        'id',
        'recipient_name as name',
        'recipient_email as email',
        'unit_labels as unitLabels',
        'status',
        'attempts',
        'last_error as lastError',
        'sent_at as sentAt'
      )
    return { announcement, jobs, units }
  }
}
