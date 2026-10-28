import type { HttpContext } from '@adonisjs/core/http'
import vine from '@vinejs/vine'
import ResidencyService from '../residency/application/residency_service.js'
import ResidencyRepository from '../residency/infrastructure/residency_repository.js'
import { auditRequest, quote, ref, type AuditTrail } from '../audit/audit.js'

const unitValidator = vine.compile(
  vine.object({
    label: vine.string().trim().minLength(1).maxLength(80),
    floor: vine.string().trim().minLength(1).maxLength(40),
  })
)
const onboardingValidator = vine.compile(
  vine.object({
    unitId: vine.number().positive().withoutDecimals(),
    residentId: vine.number().positive().withoutDecimals().optional(),
    name: vine.string().trim().minLength(1).maxLength(160).optional(),
    email: vine.string().trim().email().maxLength(254).optional(),
    phone: vine.string().trim().maxLength(40).optional(),
    role: vine.enum(['owner', 'tenant', 'occupant']),
    startDate: vine.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  })
)
const offboardingValidator = vine.compile(
  vine.object({
    endDate: vine.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  })
)

function isHandledFailure(error: unknown) {
  const failure = error as { code?: string; status?: number }
  return (
    failure.code === '23505' ||
    Boolean(failure.status && [404, 409, 422].includes(failure.status)) ||
    (error instanceof Error && error.name === 'OccupancyValidationError')
  )
}

async function showFailure(error: unknown, ctx: HttpContext, trail: AuditTrail, intent: string) {
  const { session, response } = ctx
  const failure = error as { code?: string; status?: number; message?: string }
  await trail.failed(intent, error, { unexpected: !isHandledFailure(error) })
  if (failure.code === '23505') {
    session.flash('error', 'Ja existe uma fracao ou relacao ativa com estes dados.')
  } else if (failure.status && [404, 409, 422].includes(failure.status)) {
    session.flash('error', failure.message || 'Nao foi possivel concluir a operacao.')
  } else if (error instanceof Error && error.name === 'OccupancyValidationError') {
    session.flash('error', error.message)
  } else {
    throw error
  }
  return response.redirect().back()
}

export default class ResidencyController {
  async index({ inertia, request }: HttpContext) {
    const data = await new ResidencyRepository().list()
    const view =
      request.url() === '/units'
        ? 'units'
        : request.url() === '/residents'
          ? 'residents'
          : 'overview'
    return inertia.render('residency/index', { ...data, view })
  }

  async createUnit(ctx: HttpContext) {
    const trail = auditRequest(ctx, 'residency.unit.create')
    const [error, input] = await unitValidator.tryValidate(ctx.request.all())
    if (error) {
      await trail.failed('create a unit', error)
      ctx.session.flash('errors', {
        form: 'Preencha a designacao e o piso (max. 80 e 40 caracteres).',
      })
      return ctx.response.redirect().back()
    }
    const intent = `create unit ${quote(input.label)} on floor ${quote(input.floor)}`
    try {
      const unitId = await new ResidencyService().createUnit(ctx.auth.user!.id, input)
      await trail.succeeded(
        `created unit #${unitId} ${quote(input.label)} on floor ${quote(input.floor)}`,
        { details: { unitId } }
      )
      ctx.session.flash('success', 'Fracao criada.')
      return ctx.response.redirect('/units')
    } catch (failure) {
      return showFailure(failure, ctx, trail, intent)
    }
  }

  async onboard(ctx: HttpContext) {
    const trail = auditRequest(ctx, 'residency.resident.onboard')
    const [error, input] = await onboardingValidator.tryValidate(ctx.request.all())
    if (error) {
      await trail.failed('onboard a resident', error)
      ctx.session.flash('errors', { form: 'Verifique os campos, o email e a data de entrada.' })
      return ctx.response.redirect().back()
    }
    const resident = input.residentId ? `existing resident #${input.residentId}` : 'a new resident'
    const intent = `onboard ${resident} to unit #${input.unitId} as ${input.role} from ${input.startDate}`
    try {
      const occupancyId = await new ResidencyService().onboard(ctx.auth.user!.id, input)
      await trail.succeeded(
        `onboarded ${resident} to unit #${input.unitId} as ${input.role} from ${input.startDate} (occupancy #${occupancyId})`,
        { details: { occupancyId, unitId: input.unitId, residentId: input.residentId ?? null } }
      )
      ctx.session.flash('success', 'Residente associado a fracao.')
      return ctx.response.redirect('/residents')
    } catch (failure) {
      return showFailure(failure, ctx, trail, intent)
    }
  }

  async offboard(ctx: HttpContext) {
    const trail = auditRequest(ctx, 'residency.resident.offboard')
    const [error, input] = await offboardingValidator.tryValidate(ctx.request.all())
    if (error || !/^\d+$/.test(ctx.params.id)) {
      await trail.failed(
        `offboard occupancy ${ref(ctx.params.id)}`,
        error ?? 'invalid occupancy identifier'
      )
      ctx.session.flash('errors', { form: 'Indique uma data de saida valida.' })
      return ctx.response.redirect().back()
    }
    const occupancyId = Number(ctx.params.id)
    const intent = `offboard occupancy #${occupancyId} on ${input.endDate}`
    try {
      await new ResidencyService().offboard(ctx.auth.user!.id, occupancyId, input.endDate)
      await trail.succeeded(`offboarded occupancy #${occupancyId} on ${input.endDate}`, {
        details: { occupancyId },
      })
      ctx.session.flash('success', 'Saida registada. O historico foi preservado.')
      return ctx.response.redirect('/residents')
    } catch (failure) {
      return showFailure(failure, ctx, trail, intent)
    }
  }
}
