import { access, appendFile, mkdir, readFile, rename, stat } from 'node:fs/promises'
import { join } from 'node:path'

export type AuditOutcome = 'success' | 'failure'

export interface AuditActor {
  type: 'user' | 'anonymous' | 'console'
  id?: number
  email?: string
}

export interface AuditRecord {
  event: string
  outcome: AuditOutcome
  message: string
  actor: AuditActor
  failure?: { kind: string; reason: string; status?: number }
  context?: Record<string, unknown>
}

export interface AuditEntry extends AuditRecord {
  timestamp: string
}

export interface RotatingAuditLogOptions {
  directory: string
  maxBytes: number
  prefix?: string
  now?: () => Date
}

export const MIN_AUDIT_LOG_BYTES = 4096

function isMissing(error: unknown) {
  return (error as NodeJS.ErrnoException).code === 'ENOENT'
}

/**
 * ISO-8601 timestamp with ':' replaced so it is a valid file name on every platform
 * while still sorting chronologically.
 */
export function fileTimestamp(iso: string) {
  return iso.replaceAll(':', '-')
}

/**
 * Append-only JSON Lines audit log. Entries go to `<prefix>-current.log`; once the next
 * entry would push it past `maxBytes`, the file is sealed and renamed to
 * `<prefix>-<first entry timestamp>_<last entry timestamp>.log`, so every sealed file is
 * at most `maxBytes` and within one entry of that size. Sizes and boundary timestamps
 * are read from disk on each rotation, so restarts and other processes sharing the
 * directory keep producing consistent files.
 */
export class RotatingAuditLog {
  readonly directory: string
  readonly maxBytes: number
  readonly prefix: string
  readonly #now: () => Date
  #queue: Promise<unknown> = Promise.resolve()

  constructor(options: RotatingAuditLogOptions) {
    if (!Number.isSafeInteger(options.maxBytes) || options.maxBytes < MIN_AUDIT_LOG_BYTES) {
      throw new Error(`Audit log maxBytes must be an integer of at least ${MIN_AUDIT_LOG_BYTES}.`)
    }
    this.directory = options.directory
    this.maxBytes = options.maxBytes
    this.prefix = options.prefix ?? 'audit'
    this.#now = options.now ?? (() => new Date())
  }

  get activePath() {
    return join(this.directory, `${this.prefix}-current.log`)
  }

  /**
   * Timestamps and appends an entry. Writes are serialized so rotation decisions never
   * interleave. The returned promise rejects if the entry could not be persisted.
   */
  write(record: AuditRecord): Promise<AuditEntry> {
    return this.#enqueue(async () => {
      const entry: AuditEntry = { timestamp: this.#now().toISOString(), ...record }
      await this.#append(`${JSON.stringify(entry)}\n`)
      return entry
    })
  }

  /**
   * Seals the active file regardless of its size and returns the sealed path, if any.
   */
  flush(): Promise<string | null> {
    return this.#enqueue(() => this.#seal())
  }

  #enqueue<T>(task: () => Promise<T>): Promise<T> {
    const result = this.#queue.then(task)
    this.#queue = result.catch(() => {})
    return result
  }

  async #append(line: string) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 })
    const size = await stat(this.activePath).then(
      (stats) => stats.size,
      (error) => {
        if (isMissing(error)) return 0
        throw error
      }
    )
    if (size > 0 && size + Buffer.byteLength(line) > this.maxBytes) await this.#seal()
    await appendFile(this.activePath, line, { encoding: 'utf8', mode: 0o600 })
  }

  async #seal(): Promise<string | null> {
    let content: string
    try {
      content = await readFile(this.activePath, 'utf8')
    } catch (error) {
      if (isMissing(error)) return null
      throw error
    }
    const lines = content.split('\n').filter((line) => line.trim())
    if (!lines.length) return null
    const stats = await stat(this.activePath)
    const fallback = stats.mtime.toISOString()
    const start = this.#timestampOf(lines[0]) ?? fallback
    const end = this.#timestampOf(lines[lines.length - 1]) ?? fallback
    const target = await this.#availableName(`${fileTimestamp(start)}_${fileTimestamp(end)}`)
    try {
      await rename(this.activePath, target)
    } catch (error) {
      // Another process sharing the directory sealed the file first.
      if (isMissing(error)) return null
      throw error
    }
    return target
  }

  #timestampOf(line: string): string | null {
    try {
      const { timestamp } = JSON.parse(line) as { timestamp?: unknown }
      return typeof timestamp === 'string' && !Number.isNaN(Date.parse(timestamp))
        ? new Date(timestamp).toISOString()
        : null
    } catch {
      return null
    }
  }

  async #availableName(range: string) {
    for (let attempt = 0; ; attempt++) {
      const suffix = attempt ? `.${attempt}` : ''
      const candidate = join(this.directory, `${this.prefix}-${range}${suffix}.log`)
      try {
        await access(candidate)
      } catch (error) {
        if (isMissing(error)) return candidate
        throw error
      }
    }
  }
}
