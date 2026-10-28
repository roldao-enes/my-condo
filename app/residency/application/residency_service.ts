import { Exception } from '@adonisjs/core/exceptions'
import db from '@adonisjs/lucid/services/db'
import { DateTime } from 'luxon'
import { validateOccupancyDates, type ResidentRole } from '../domain/occupancy.js'

export default class ResidencyService {
  async createUnit(actorId: number, input: { label: string; floor: string }) {
    return db.transaction(async (trx) => {
      const [unit] = await trx.table('units').insert(input).returning('id')
      const event = {
        actor_id: actorId,
        action: 'unit.created',
        entity_type: 'unit',
        entity_id: unit.id,
      }
      await trx.table('audit_events').insert(event)
      return unit.id as number
    })
  }

  async onboard(
    actorId: number,
    input: {
      unitId: number
      residentId?: number
      name?: string
      email?: string | null
      phone?: string | null
      role: ResidentRole
      startDate: string
    }
  ) {
    validateOccupancyDates(input.startDate)
    const today = DateTime.now().setZone('Europe/Lisbon').toISODate()!
    if (input.startDate > today)
      throw new Exception('A entrada nao pode ser futura.', { status: 422 })
    if (!input.residentId && !input.name?.trim())
      throw new Exception('Indique o nome do residente.', { status: 422 })

    return db.transaction(async (trx) => {
      const unit = await trx.from('units').where('id', input.unitId).first()
      if (!unit) throw new Exception('Fracao nao encontrada.', { status: 404 })
      let residentId = input.residentId
      if (residentId) {
        const resident = await trx.from('residents').where('id', residentId).first()
        if (!resident) throw new Exception('Residente nao encontrado.', { status: 404 })
      } else {
        const [resident] = await trx
          .table('residents')
          .insert({
            name: input.name!.trim(),
            email: input.email || null,
            phone: input.phone || null,
          })
          .returning('id')
        residentId = resident.id
      }
      const [occupancy] = await trx
        .table('occupancies')
        .insert({
          unit_id: input.unitId,
          resident_id: residentId,
          role: input.role,
          start_date: input.startDate,
        })
        .returning('id')
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'resident.onboarded',
        entity_type: 'occupancy',
        entity_id: occupancy.id,
        metadata: JSON.stringify({
          unitId: input.unitId,
          residentId,
          role: input.role,
          startDate: input.startDate,
        }),
      })
      return occupancy.id as number
    })
  }

  async offboard(actorId: number, occupancyId: number, endDate: string) {
    return db.transaction(async (trx) => {
      const relationship = await trx
        .from('occupancies')
        .where('id', occupancyId)
        .select('id', 'end_date')
        .select(db.raw("to_char(start_date, 'YYYY-MM-DD') as start_date"))
        .forUpdate()
        .first()
      if (!relationship) throw new Exception('Relacao nao encontrada.', { status: 404 })
      if (relationship.end_date)
        throw new Exception('Esta relacao ja foi encerrada.', { status: 409 })
      validateOccupancyDates(relationship.start_date, endDate)
      if (endDate > DateTime.now().setZone('Europe/Lisbon').toISODate()!) {
        throw new Exception('A saida nao pode ser futura.', { status: 422 })
      }
      await trx.from('occupancies').where('id', occupancyId).update({ end_date: endDate })
      await trx.table('audit_events').insert({
        actor_id: actorId,
        action: 'resident.offboarded',
        entity_type: 'occupancy',
        entity_id: occupancyId,
        metadata: JSON.stringify({ endDate }),
      })
    })
  }
}
