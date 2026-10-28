import db from '@adonisjs/lucid/services/db'

export default class ResidencyRepository {
  async list() {
    const units = await db.from('units').select('id', 'label', 'floor').orderBy('label')
    const residents = await db
      .from('residents')
      .select('id', 'name', 'email', 'phone')
      .orderBy('name')
    const relationships = await db
      .from('occupancies')
      .join('residents', 'residents.id', 'occupancies.resident_id')
      .join('units', 'units.id', 'occupancies.unit_id')
      .select(
        'occupancies.id',
        'unit_id',
        'resident_id',
        'role',
        'name',
        'email',
        'phone',
        'units.label as unit_label'
      )
      .select(db.raw("to_char(start_date, 'YYYY-MM-DD') as start_date"))
      .select(db.raw("to_char(end_date, 'YYYY-MM-DD') as end_date"))
      .orderByRaw('COALESCE(end_date, start_date) DESC')
      .orderBy('occupancies.id', 'desc')
    return { units, residents, relationships }
  }
}
