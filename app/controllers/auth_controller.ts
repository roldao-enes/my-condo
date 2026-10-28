import type { HttpContext } from '@adonisjs/core/http'
import vine from '@vinejs/vine'
import User from '#models/user'

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

  async store({ request, auth, response, session }: HttpContext) {
    const [error, input] = await loginValidator.tryValidate(request.all())
    if (error) {
      session.flash('errors', { email: 'Verifique o email e a palavra-passe.' })
      return response.redirect('/login')
    }
    let user: User
    try {
      user = await User.verifyCredentials(input.email, input.password)
    } catch (failure) {
      if ((failure as { code?: string }).code !== 'E_INVALID_CREDENTIALS') throw failure
      session.flash('errors', { email: 'Email ou palavra-passe incorretos.' })
      return response.redirect('/login')
    }
    await auth.use('web').login(user)
    return response.redirect('/')
  }

  async destroy({ auth, response }: HttpContext) {
    await auth.use('web').logout()
    return response.redirect('/login')
  }
}
