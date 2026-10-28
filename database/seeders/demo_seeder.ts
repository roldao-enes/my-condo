import { BaseSeeder } from '@adonisjs/lucid/seeders'
import app from '@adonisjs/core/services/app'
import db from '@adonisjs/lucid/services/db'
import User from '#models/user'
import ResidencyService from '../../app/residency/application/residency_service.js'

export default class extends BaseSeeder {
  async run() {
    if (!app.inDev) throw new Error('Demo data is only allowed in development.')
    if ((await User.query().first()) || (await db.from('units').first())) {
      throw new Error('Demo seed requires an empty database. Existing data was not changed.')
    }
    const actor = await User.create({
      fullName: 'Ana Martins',
      email: 'admin@example.test',
      password: 'DemoCondo2026!',
    })
    const service = new ResidencyService()
    const entries = [
      { label: 'R/C Esq.', floor: '0', name: 'Maria Silva', role: 'owner' as const },
      { label: 'R/C Dir.', floor: '0', name: 'Pedro Costa', role: 'tenant' as const },
      { label: '1.o Esq.', floor: '1', name: 'Sofia Almeida', role: 'owner' as const },
      { label: '1.o Dir.', floor: '1', name: 'Miguel Santos', role: 'owner' as const },
      { label: '2.o Esq.', floor: '2', name: 'Beatriz Ferreira', role: 'tenant' as const },
      { label: '2.o Dir.', floor: '2', name: null, role: 'owner' as const },
    ]
    for (const [index, entry] of entries.entries()) {
      const unitId = await service.createUnit(actor.id, { label: entry.label, floor: entry.floor })
      if (entry.name)
        await service.onboard(actor.id, {
          unitId,
          name: entry.name,
          email: `resident${index + 1}@example.test`,
          role: entry.role,
          startDate: '2025-01-01',
        })
    }
  }
}
