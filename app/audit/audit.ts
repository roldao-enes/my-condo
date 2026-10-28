import { isAbsolute } from 'node:path'
import app from '@adonisjs/core/services/app'
import logger from '@adonisjs/core/services/logger'
import type { HttpContext } from '@adonisjs/core/http'
import env from '#start/env'
import { RotatingAuditLog, type AuditActor, type AuditRecord } from './rotating_audit_log.js'

export const DEFAULT_AUDIT_LOG_MAX_BYTES = 1024 * 1024

let instance: RotatingAuditLog | undefined

export function auditLog() {
  if (!instance) {
    const directory = env.get('AUDIT_LOG_DIR')
    instance = new RotatingAuditLog({
      directory: directory
        ? isAbsolute(directory)
          ? directory
          : app.makePath(directory)
        : app.tmpPath('audit'),
      maxBytes: env.get('AUDIT_LOG_MAX_BYTES') ?? DEFAULT_AUDIT_LOG_MAX_BYTES,
    })
  }
  return instance
}

/**
 * Persists an audit record and mirrors it to the application logger so it also reaches
 * the platform log pipeline. Audit write failures are reported but never break the
 * operation being audited.
 */
export async function recordAudit(record: AuditRecord) {
  if (record.outcome === 'success') logger.info({ audit: record }, record.message)
  else logger.warn({ audit: record }, record.message)
  try {
    await auditLog().write(record)
  } catch (error) {
    logger.error({ err: error, audit: record }, 'Failed to write audit log entry')
  }
}

export function quote(value: unknown, max = 120) {
  const text = String(value ?? '')
  return JSON.stringify(text.length > max ? `${text.slice(0, max)}...` : text)
}

/**
 * Formats a route parameter as an entity reference without trusting its content.
 */
export function ref(value: unknown) {
  return typeof value === 'string' && /^\d+$/.test(value) ? `#${value}` : quote(value, 40)
}

export function actorLabel(actor: AuditActor) {
  if (actor.type === 'console') return 'Console operator'
  if (actor.type === 'anonymous') return 'Anonymous visitor'
  return `User #${actor.id}${actor.email ? ` (${actor.email})` : ''}`
}

function userActor(user?: { id: number; email: string } | null): AuditActor {
  return user ? { type: 'user', id: user.id, email: user.email } : { type: 'anonymous' }
}

export type FailureKind =
  'invalid_input' | 'invalid_credentials' | 'not_found' | 'conflict' | 'rejected' | 'error'

/**
 * Marks a user-facing error caused by malformed request input.
 */
export class InvalidInputError extends Error {
  name = 'InvalidInputError'
}

export interface FailureDescription {
  kind: FailureKind
  reason: string
  status?: number
}

/**
 * Translates an error into an explicit failure mode. A string describes an input problem
 * detected by the caller. `unexpected` marks errors that were not handled for the user
 * (they surface as server errors) so they are not mistaken for business rejections.
 */
export function describeFailure(error: unknown, unexpected = false): FailureDescription {
  if (typeof error === 'string') return { kind: 'invalid_input', reason: error, status: 422 }
  const failure = (error ?? {}) as {
    code?: string
    status?: number
    message?: string
    name?: string
    constraint?: string
    messages?: Array<{ field?: string }>
  }
  const detail = String(failure.message ?? error).slice(0, 300)
  if (failure.code === 'E_VALIDATION_ERROR') {
    const fields = [...new Set((failure.messages ?? []).map((item) => item.field).filter(Boolean))]
    return {
      kind: 'invalid_input',
      reason: `invalid input in ${fields.length ? `field(s) ${fields.join(', ')}` : 'the submitted form'}`,
      status: 422,
    }
  }
  if (error instanceof InvalidInputError) {
    return { kind: 'invalid_input', reason: `invalid input: ${detail}`, status: 422 }
  }
  if (failure.code === 'E_INVALID_CREDENTIALS') {
    return { kind: 'invalid_credentials', reason: 'invalid email or password', status: 400 }
  }
  if (failure.code === '23505') {
    return {
      kind: 'conflict',
      reason: `duplicate record rejected by the database${failure.constraint ? ` (${failure.constraint})` : ''}`,
      status: 409,
    }
  }
  if (failure.status === 404)
    return { kind: 'not_found', reason: `not found: ${detail}`, status: 404 }
  if (failure.status === 409)
    return { kind: 'conflict', reason: `conflict: ${detail}`, status: 409 }
  if (
    !unexpected &&
    ((failure.status && failure.status < 500) ||
      error instanceof Error ||
      failure.name === 'OccupancyValidationError')
  ) {
    return {
      kind: 'rejected',
      reason: `rejected by business rule: ${detail}`,
      status: failure.status ?? 422,
    }
  }
  const name = failure.name ?? 'Error'
  return {
    kind: 'error',
    reason: `unexpected error ${name}${failure.code ? ` [${failure.code}]` : ''}: ${detail}`,
    status: failure.status && failure.status >= 500 ? failure.status : 500,
  }
}

/**
 * Audit trail for one state-changing HTTP request. The acting user is captured when the
 * trail is created (before logout clears it); login passes the authenticated user.
 */
export function auditRequest(ctx: HttpContext, event: string) {
  const actor = userActor(ctx.auth.user)
  const context = {
    requestId: ctx.request.id(),
    ip: ctx.request.ip(),
    method: ctx.request.method(),
    url: ctx.request.url(),
  }
  return {
    succeeded(
      description: string,
      options: { actor?: { id: number; email: string }; details?: Record<string, unknown> } = {}
    ) {
      const who = options.actor ? userActor(options.actor) : actor
      return recordAudit({
        event,
        outcome: 'success',
        message: `${actorLabel(who)} ${description}`,
        actor: who,
        context: { ...context, ...options.details },
      })
    },
    failed(
      intent: string,
      error: unknown,
      options: { unexpected?: boolean; details?: Record<string, unknown> } = {}
    ) {
      const failure = describeFailure(error, options.unexpected)
      return recordAudit({
        event,
        outcome: 'failure',
        message: `${actorLabel(actor)} failed to ${intent}: ${failure.reason}`,
        actor,
        failure,
        context: { ...context, ...options.details },
      })
    },
  }
}

export type AuditTrail = ReturnType<typeof auditRequest>
