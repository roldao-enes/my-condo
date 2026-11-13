import type { HttpContext } from '@adonisjs/core/http'
import vine from '@vinejs/vine'
import AnnouncementService from '../mail/application/announcement_service.js'
import { auditRequest, quote, ref, type AuditTrail } from '../audit/audit.js'

const draftValidator = vine.compile(
  vine.object({
    subject: vine.string().trim().minLength(1).maxLength(200),
    body: vine.string().trim().minLength(1).maxLength(10000),
  })
)
const queueValidator = vine.compile(
  vine.object({
    unitId: vine.number().positive().withoutDecimals().optional(),
  })
)

function isHandledFailure(error: unknown) {
  const failure = error as { status?: number }
  return Boolean(failure.status && [404, 409, 422].includes(failure.status))
}

async function showFailure(error: unknown, ctx: HttpContext, trail: AuditTrail, intent: string) {
  await trail.failed(intent, error, { unexpected: !isHandledFailure(error) })
  if (!isHandledFailure(error)) throw error
  ctx.session.flash(
    'error',
    (error as { message?: string }).message || 'Nao foi possivel concluir a operacao.'
  )
  return ctx.response.redirect().back()
}

export default class AnnouncementsController {
  async index({ inertia }: HttpContext) {
    return inertia.render('announcements/index', {
      announcements: await new AnnouncementService().list(),
    })
  }

  async store(ctx: HttpContext) {
    const trail = auditRequest(ctx, 'mail.announcement.create')
    const [error, input] = await draftValidator.tryValidate(ctx.request.all())
    if (error) {
      await trail.failed('create an announcement', error)
      ctx.session.flash('errors', {
        form: 'Indique o assunto (max. 200 caracteres) e a mensagem (max. 10000 caracteres).',
      })
      return ctx.response.redirect().back()
    }
    const intent = `create announcement ${quote(input.subject)}`
    try {
      const id = await new AnnouncementService().createDraft(ctx.auth.user!.id, input)
      await trail.succeeded(`created announcement #${id} ${quote(input.subject)}`, {
        details: { announcementId: id },
      })
      ctx.session.flash('success', 'Rascunho criado. Reveja e coloque em fila para envio.')
      return ctx.response.redirect(`/announcements/${id}`)
    } catch (failure) {
      return showFailure(failure, ctx, trail, intent)
    }
  }

  async show(ctx: HttpContext) {
    if (!/^\d+$/.test(ctx.params.id)) return ctx.response.notFound()
    try {
      const data = await new AnnouncementService().show(Number(ctx.params.id))
      return ctx.inertia.render('announcements/show', data)
    } catch (failure) {
      if ((failure as { status?: number }).status === 404) return ctx.response.notFound()
      throw failure
    }
  }

  async queue(ctx: HttpContext) {
    const trail = auditRequest(ctx, 'mail.announcement.queue')
    const [error, input] = await queueValidator.tryValidate(ctx.request.all())
    if (error || !/^\d+$/.test(ctx.params.id)) {
      await trail.failed(
        `queue announcement ${ref(ctx.params.id)}`,
        error ?? 'invalid announcement identifier'
      )
      ctx.session.flash('errors', { form: 'Pedido invalido.' })
      return ctx.response.redirect().back()
    }
    const announcementId = Number(ctx.params.id)
    const intent = `queue announcement #${announcementId}`
    try {
      const recipients = await new AnnouncementService().queue(ctx.auth.user!.id, announcementId, {
        unitId: input.unitId,
      })
      await trail.succeeded(`queued announcement #${announcementId} for ${recipients} recipients`, {
        details: { announcementId, recipients, unitId: input.unitId ?? null },
      })
      ctx.session.flash(
        'success',
        `${recipients} destinatario(s) em fila. Execute "node ace send:emails" para enviar.`
      )
      return ctx.response.redirect(`/announcements/${announcementId}`)
    } catch (failure) {
      return showFailure(failure, ctx, trail, intent)
    }
  }
}
