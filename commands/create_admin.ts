import { BaseCommand, flags } from '@adonisjs/core/ace'
import type { CommandOptions } from '@adonisjs/core/types/ace'
import User from '#models/user'
import { describeFailure, quote, recordAudit } from '../app/audit/audit.js'
import type { AuditActor } from '../app/audit/rotating_audit_log.js'

const EVENT = 'admin.create'
const ACTOR: AuditActor = { type: 'console' }

export default class CreateAdmin extends BaseCommand {
  static commandName = 'create:admin'
  static description = 'Create an administrator (no public registration)'
  static options: CommandOptions = { startApp: true }

  @flags.boolean({
    description: 'Read ADMIN_NAME, ADMIN_EMAIL, and ADMIN_PASSWORD from the environment',
  })
  declare fromEnv: boolean

  async run() {
    const fullName = this.fromEnv
      ? process.env.ADMIN_NAME?.trim() || 'Administrador'
      : await this.prompt.ask('Administrator name')
    const enteredEmail = this.fromEnv
      ? process.env.ADMIN_EMAIL || ''
      : await this.prompt.ask('Email')
    const email = enteredEmail.trim().toLowerCase()
    const password = this.fromEnv
      ? process.env.ADMIN_PASSWORD || ''
      : await this.prompt.secure('Password (at least 12 characters)')
    if (
      !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email) ||
      email.length > 254 ||
      password.length < 12 ||
      !fullName.trim() ||
      fullName.length > 160
    ) {
      await this.#audit(
        'create an administrator',
        'invalid input: a valid email, a name of at most 160 characters and a password of at least 12 characters are required'
      )
      this.logger.error('A valid email and a password of at least 12 characters are required.')
      this.exitCode = 1
      return
    }
    const intent = `create administrator ${quote(email, 254)}`
    try {
      if (await User.findBy('email', email)) {
        await this.#audit(intent, {
          kind: 'conflict',
          reason: 'conflict: an account with this email already exists; no account was changed',
          status: 409,
        })
        this.logger.error(
          'An administrator with this email already exists. No account was changed.'
        )
        this.exitCode = 1
        return
      }
      const user = await User.create({ fullName, email, password })
      await recordAudit({
        event: EVENT,
        outcome: 'success',
        message: `Console operator created administrator #${user.id} (${email}) via create:admin${this.fromEnv ? ' --from-env' : ''}`,
        actor: ACTOR,
        context: { userId: user.id, fromEnv: Boolean(this.fromEnv) },
      })
      this.logger.success('Administrator created.')
    } catch (error) {
      await this.#audit(intent, describeFailure(error, true))
      throw error
    }
  }

  async #audit(intent: string, failure: string | ReturnType<typeof describeFailure>) {
    const described = typeof failure === 'string' ? describeFailure(failure) : failure
    await recordAudit({
      event: EVENT,
      outcome: 'failure',
      message: `Console operator failed to ${intent}: ${described.reason}`,
      actor: ACTOR,
      failure: described,
      context: { fromEnv: Boolean(this.fromEnv) },
    })
  }
}
