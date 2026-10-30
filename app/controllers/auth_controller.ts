import type { HttpContext } from '@adonisjs/core/http'
import vine from '@vinejs/vine'
import User from '#models/user'
import { auditRequest, quote } from '../audit/audit.js'

const loginValidator = vine.compile(
  vine.object({
    email: vine.string().trim().email().normalizeEmail(),
    password: vine.string().minLength(1).maxLength(256),
  })
)

export default class AuthController {
  async show({ inertia }: HttpContext) {
    return inertia.render('auth/login')
  }

  async store(ctx: HttpContext) {
    const { request, auth, response, session } = ctx
    const trail = auditRequest(ctx, 'auth.login')
    const [error, input] = await loginValidator.tryValidate(request.all())
    if (error) {
      await trail.failed('log in', error)
      session.flashErrors({ email: 'Verifique o email e a palavra-passe.' })
      return response.redirect('/login')
    }
    const intent = `log in as ${quote(input.email, 254)}`
    let user: User
    try {
      user = await User.verifyCredentials(input.email, input.password)
    } catch (failure) {
      const unexpected = (failure as { code?: string }).code !== 'E_INVALID_CREDENTIALS'
      await trail.failed(intent, failure, { unexpected })
      if (unexpected) throw failure
      session.flashErrors({ email: 'Email ou palavra-passe incorretos.' })
      return response.redirect('/login')
    }
    try {
      await auth.use('web').login(user)
    } catch (failure) {
      await trail.failed(intent, failure, { unexpected: true })
      throw failure
    }
    await trail.succeeded('logged in', { actor: user })
    return response.redirect('/')
  }

  async destroy(ctx: HttpContext) {
    const trail = auditRequest(ctx, 'auth.logout')
    try {
      await ctx.auth.use('web').logout()
    } catch (failure) {
      await trail.failed('log out', failure, { unexpected: true })
      throw failure
    }
    await trail.succeeded('logged out')
    return ctx.response.redirect('/login')
  }
}
