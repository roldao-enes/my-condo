import { readFile, unlink } from 'node:fs/promises'
import type { HttpContext } from '@adonisjs/core/http'
import vine from '@vinejs/vine'
import { DateTime } from 'luxon'
import FinanceService from '../finances/finance_service.js'
import { parseMoney } from '../finances/domain/statement.js'
import { auditRequest, InvalidInputError, quote, ref } from '../audit/audit.js'

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
    remember: vine.boolean().optional(),
  })
)
const categoryValidator = vine.compile(
  vine.object({
    category: vine.enum(['unclassified', 'recurring', 'extraordinary']),
    remember: vine.boolean().optional(),
  })
)
const tagValidator = vine.compile(
  vine.object({
    name: vine.string().trim().minLength(1).maxLength(160),
    pattern: vine.string().trim().minLength(3).maxLength(500),
    matchType: vine.enum(['exact', 'contains']),
    category: vine.enum(['recurring', 'extraordinary', 'resident_payment']),
    residentId: vine.number().positive().withoutDecimals().nullable().optional(),
    unitId: vine.number().positive().withoutDecimals().nullable().optional(),
  })
)
const identityValidator = vine.compile(
  vine.object({
    residentId: vine.number().positive().withoutDecimals().nullable(),
    unitId: vine.number().positive().withoutDecimals().nullable(),
    remember: vine.boolean().optional(),
  })
)

interface Outcome {
  notice: string
  audit: string
  details?: Record<string, unknown>
}

async function mutate(
  ctx: HttpContext,
  event: string,
  intent: string,
  action: () => Promise<Outcome>
) {
  const trail = auditRequest(ctx, event)
  try {
    const outcome = await action()
    await trail.succeeded(outcome.audit, { details: outcome.details })
    ctx.session.flash('success', outcome.notice)
  } catch (error) {
    const failure = error as { status?: number; code?: string; message: string }
    const unexpected =
      failure.code !== '23505' &&
      ((failure.status !== undefined && failure.status >= 500) ||
        (failure.code !== undefined && failure.code !== 'E_VALIDATION_ERROR') ||
        !(error instanceof Error))
    await trail.failed(intent, error, { unexpected })
    if (failure.code === '23505') {
      ctx.session.flash(
        'error',
        'Ja existe uma etiqueta com este nome de correspondencia e direcao.'
      )
      return ctx.response.redirect('/finances')
    }
    if (unexpected) throw error
    ctx.session.flash(
      'error',
      failure.code === 'E_VALIDATION_ERROR' ? 'Verifique os campos do formulario.' : failure.message
    )
  }
  return ctx.response.redirect('/finances')
}

function id(value: string) {
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)))
    throw new InvalidInputError('Identificador invalido.')
  return Number(value)
}

function money(value: string) {
  try {
    return parseMoney(value)
  } catch (error) {
    throw new InvalidInputError((error as Error).message)
  }
}

function euros(cents: number) {
  return `EUR ${(cents / 100).toFixed(2)}`
}

export default class FinancesController {
  async index({ inertia }: HttpContext) {
    return inertia.render('finances/index', await new FinanceService().list())
  }

  async import(ctx: HttpContext) {
    return mutate(ctx, 'finance.statement.import', 'import a bank statement', async () => {
      const file = ctx.request.file('statement', { size: '2mb', extnames: ['csv', 'tsv', 'txt'] })
      try {
        if (!file?.isValid || !file.tmpPath)
          throw new InvalidInputError('Selecione um ficheiro CSV valido (max. 2 MB).')
        const result = await new FinanceService().importStatement(
          ctx.auth.user!.id,
          await readFile(file.tmpPath, 'utf8')
        )
        return {
          notice: `${result.imported} movimentos importados; ${result.skipped} duplicados ignorados.`,
          audit: `imported bank statement ${quote(file.clientName)}: ${result.imported} movement(s) added, ${result.skipped} duplicate(s) skipped`,
          details: { ...result, file: file.clientName, bytes: file.size },
        }
      } finally {
        if (file?.tmpPath) await unlink(file.tmpPath).catch(() => {})
      }
    })
  }

  async classify(ctx: HttpContext) {
    const target = `transaction ${ref(ctx.params.id)}`
    return mutate(ctx, 'finance.transaction.classify', `classify ${target}`, async () => {
      const input = await categoryValidator.validate(ctx.request.all())
      const remember = input.remember !== false
      await new FinanceService().classify(
        ctx.auth.user!.id,
        id(ctx.params.id),
        input.category,
        remember
      )
      return {
        notice: 'Classificacao atualizada.',
        audit: `classified ${target} as ${input.category}${remember ? ' and remembered the rule for future imports' : ''}`,
        details: { transactionId: id(ctx.params.id), category: input.category, remember },
      }
    })
  }

  async charge(ctx: HttpContext) {
    return mutate(ctx, 'finance.charge.create', 'register a charge', async () => {
      const input = await chargeValidator.validate(ctx.request.all())
      const amount = money(input.amount)
      if (amount <= 0 || !DateTime.fromISO(input.dueDate).isValid)
        throw new InvalidInputError('Indique um montante positivo e uma data valida.')
      const chargeId = await new FinanceService().createCharge(ctx.auth.user!.id, {
        ...input,
        amount,
      })
      return {
        notice: 'Quota registada.',
        audit: `registered charge #${chargeId} ${quote(input.description)} of ${euros(amount)} due ${input.dueDate} for resident #${input.residentId} in unit #${input.unitId}`,
        details: { chargeId, residentId: input.residentId, unitId: input.unitId, amount },
      }
    })
  }

  async allocate(ctx: HttpContext) {
    const target = `transaction ${ref(ctx.params.id)}`
    return mutate(
      ctx,
      'finance.payment.allocate',
      `allocate a payment from ${target}`,
      async () => {
        const input = await paymentValidator.validate(ctx.request.all())
        const amount = money(input.amount)
        const remember = input.remember !== false
        await new FinanceService().allocate(
          ctx.auth.user!.id,
          id(ctx.params.id),
          input.chargeId,
          amount,
          remember
        )
        return {
          notice: 'Pagamento associado.',
          audit: `allocated ${euros(amount)} from ${target} to charge #${input.chargeId}${remember ? ' and remembered the payer name' : ''}`,
          details: { transactionId: id(ctx.params.id), chargeId: input.chargeId, amount, remember },
        }
      }
    )
  }

  async unallocate(ctx: HttpContext) {
    const target = `payment allocation ${ref(ctx.params.id)}`
    return mutate(ctx, 'finance.payment.unallocate', `remove ${target}`, async () => {
      await new FinanceService().unallocate(ctx.auth.user!.id, id(ctx.params.id))
      return {
        notice: 'Associacao removida.',
        audit: `removed ${target}`,
        details: { paymentId: id(ctx.params.id) },
      }
    })
  }

  async identify(ctx: HttpContext) {
    const target = `transaction ${ref(ctx.params.id)}`
    return mutate(
      ctx,
      'finance.transaction.identify',
      `identify the payer of ${target}`,
      async () => {
        const input = await identityValidator.validate(ctx.request.all())
        const remember = input.remember !== false
        await new FinanceService().identify(
          ctx.auth.user!.id,
          id(ctx.params.id),
          input.residentId,
          input.unitId,
          remember
        )
        const payer = input.residentId
          ? `resident #${input.residentId} in unit #${input.unitId ?? 'unknown'}`
          : 'no resident (identification cleared)'
        return {
          notice: 'Identificacao do residente atualizada.',
          audit: `identified the payer of ${target} as ${payer}${remember ? ' and remembered the payer name' : ''}`,
          details: {
            transactionId: id(ctx.params.id),
            residentId: input.residentId,
            unitId: input.unitId,
            remember,
          },
        }
      }
    )
  }

  async saveTag(ctx: HttpContext) {
    const existing = ctx.params.id !== undefined
    const intent = existing ? `update tag ${ref(ctx.params.id)}` : 'create a tag'
    return mutate(ctx, existing ? 'finance.tag.update' : 'finance.tag.create', intent, async () => {
      const input = await tagValidator.validate(ctx.request.all())
      const tagId = await new FinanceService().saveTag(
        ctx.auth.user!.id,
        input,
        existing ? id(ctx.params.id) : undefined
      )
      return {
        notice: 'Etiqueta guardada.',
        audit: `${existing ? 'updated' : 'created'} tag #${tagId} ${quote(input.name)} (${input.matchType} match on ${quote(input.pattern)}, category ${input.category})`,
        details: { tagId, category: input.category, matchType: input.matchType },
      }
    })
  }

  async deleteTag(ctx: HttpContext) {
    const target = `tag ${ref(ctx.params.id)}`
    return mutate(ctx, 'finance.tag.delete', `remove ${target}`, async () => {
      await new FinanceService().deleteTag(ctx.auth.user!.id, id(ctx.params.id))
      return {
        notice: 'Etiqueta removida. Os movimentos anteriores foram preservados.',
        audit: `removed ${target}; previously tagged movements were preserved`,
        details: { tagId: id(ctx.params.id) },
      }
    })
  }
}
