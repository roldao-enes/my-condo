import { BaseCommand, flags } from '@adonisjs/core/ace'
import type { CommandOptions } from '@adonisjs/core/types/ace'
import User from '#models/user'

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
      this.logger.error('A valid email and a password of at least 12 characters are required.')
      this.exitCode = 1
      return
    }
    if (await User.findBy('email', email)) {
      this.logger.error('An administrator with this email already exists. No account was changed.')
      this.exitCode = 1
      return
    }
    await User.create({ fullName, email, password })
    this.logger.success('Administrator created.')
  }
}
