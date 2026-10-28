import type { HttpContext } from '@adonisjs/core/http'
import type { NextFn } from '@adonisjs/core/types/http'
import type { Authenticators } from '@adonisjs/auth/types'

/**
 * Auth middleware is used authenticate HTTP requests and deny
 * access to unauthenticated users.
 */
export default class AuthMiddleware {
  /**
   * The URL to redirect to, when authentication fails
   */
  redirectTo = '/login'

  async handle(
    ctx: HttpContext,
    next: NextFn,
    options: {
      guards?: (keyof Authenticators)[]
    } = {}
  ) {
    const guards = options.guards || ['web']
    let authenticated = false
    for (const guard of guards) {
      if (await ctx.auth.use(guard).check()) {
        authenticated = true
        break
      }
    }
    if (!authenticated) return ctx.response.redirect(this.redirectTo)
    await ctx.auth.authenticateUsing(options.guards)
    return next()
  }
}
