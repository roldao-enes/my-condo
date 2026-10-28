import { readFile, unlink } from 'node:fs/promises'
import type { HttpContext } from '@adonisjs/core/http'
import vine from '@vinejs/vine'
import { DateTime } from 'luxon'
import FinanceService from '../finances/finance_service.js'
import { parseMoney } from '../finances/domain/statement.js'

const chargeValidator = vine.compile(
  vine.object({
    residentId: vine.number().positive().withoutDecimals(),
    unitId: vine.number().positive().withoutDecimals(),
    description: vine.string().trim().minLength(1).maxLength(200),
    dueDate: vine.string().regex(/^\d{4}-\d{2}-\d{2}$/),
    amount: vine.string().trim(),
  })
)
const paymentValidator = vine.compile(
  vine.object({
    chargeId: vine.number().positive().withoutDecimals(),
    amount: vine.string().trim(),
  })
)
const categoryValidator = vine.compile(
  vine.object({
    category: vine.enum(['unclassified', 'recurring', 'extraordinary']),
  })
)

async function mutate(ctx: HttpContext, action: () => Promise<string>) {
  try {
    ctx.session.flash('success', await action())
  } catch (error) {
    const failure = error as { status?: number; code?: string; message: string }
    if (failure.status && failure.status >= 500) throw error
    if (failure.code && failure.code !== 'E_VALIDATION_ERROR') throw error
    if (!(error instanceof Error)) throw error
    ctx.session.flash(
      'error',
      failure.code === 'E_VALIDATION_ERROR' ? 'Verifique os campos do formulario.' : failure.message
    )
  }
  return ctx.response.redirect('/finances')
}

function id(value: string) {
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)))
    throw new Error('Identificador invalido.')
  return Number(value)
}

export default class FinancesController {
  async index({ inertia }: HttpContext) {
    return inertia.render('finances/index', await new FinanceService().list())
  }

  async import(ctx: HttpContext) {
    return mutate(ctx, async () => {
      const file = ctx.request.file('statement', { size: '2mb', extnames: ['csv', 'tsv', 'txt'] })
      try {
        if (!file?.isValid || !file.tmpPath)
          throw new Error('Selecione um ficheiro CSV valido (max. 2 MB).')
        const result = await new FinanceService().importStatement(
          ctx.auth.user!.id,
          await readFile(file.tmpPath, 'utf8')
        )
        return `${result.imported} movimentos importados; ${result.skipped} duplicados ignorados.`
      } finally {
        if (file?.tmpPath) await unlink(file.tmpPath).catch(() => {})
      }
    })
  }

  async classify(ctx: HttpContext) {
    return mutate(ctx, async () => {
      const input = await categoryValidator.validate(ctx.request.all())
      await new FinanceService().classify(ctx.auth.user!.id, id(ctx.params.id), input.category)
      return 'Classificacao atualizada.'
    })
  }

  async charge(ctx: HttpContext) {
    return mutate(ctx, async () => {
      const input = await chargeValidator.validate(ctx.request.all())
      const amount = parseMoney(input.amount)
      if (amount <= 0 || !DateTime.fromISO(input.dueDate).isValid)
        throw new Error('Indique um montante positivo e uma data valida.')
      await new FinanceService().createCharge(ctx.auth.user!.id, { ...input, amount })
      return 'Quota registada.'
    })
  }

  async allocate(ctx: HttpContext) {
    return mutate(ctx, async () => {
      const input = await paymentValidator.validate(ctx.request.all())
      await new FinanceService().allocate(
        ctx.auth.user!.id,
        id(ctx.params.id),
        input.chargeId,
        parseMoney(input.amount)
      )
      return 'Pagamento associado.'
    })
  }

  async unallocate(ctx: HttpContext) {
    return mutate(ctx, async () => {
      await new FinanceService().unallocate(ctx.auth.user!.id, id(ctx.params.id))
      return 'Associacao removida.'
    })
  }
}
