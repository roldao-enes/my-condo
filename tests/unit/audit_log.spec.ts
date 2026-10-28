import { test } from '@japa/runner'
import { mkdtemp, readdir, readFile, rm, stat, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  RotatingAuditLog,
  fileTimestamp,
  type AuditRecord,
} from '../../app/audit/rotating_audit_log.js'
import { describeFailure, InvalidInputError } from '../../app/audit/audit.js'

function clock(start = Date.parse('2026-01-01T00:00:00.000Z')) {
  let current = start
  return () => new Date((current += 1000))
}

function record(index: number): AuditRecord {
  return {
    event: 'test.event',
    outcome: 'success',
    message: `User #1 performed action number ${String(index).padStart(4, '0')}`,
    actor: { type: 'user', id: 1 },
  }
}

async function readLines(path: string) {
  const content = await readFile(path, 'utf8')
  return content.trim().split('\n')
}

const SEALED =
  /^audit-(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.\d{3}Z)_(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}\.\d{3}Z)\.log$/

test.group('Rotating audit log', (group) => {
  let directory: string
  group.each.setup(async () => {
    directory = await mkdtemp(join(tmpdir(), 'audit-log-'))
    return () => rm(directory, { recursive: true, force: true })
  })

  test('writes timestamped JSON lines with the action message', async ({ assert }) => {
    const log = new RotatingAuditLog({ directory, maxBytes: 4096, now: clock() })
    const entry = await log.write(record(1))
    assert.equal(entry.timestamp, '2026-01-01T00:00:01.000Z')
    const [line] = await readLines(log.activePath)
    assert.deepEqual(JSON.parse(line), {
      timestamp: '2026-01-01T00:00:01.000Z',
      ...record(1),
    })
  })

  test('seals evenly sized files named by their first and last entry dates', async ({ assert }) => {
    const log = new RotatingAuditLog({ directory, maxBytes: 4096, now: clock() })
    const entries = []
    for (let index = 0; index < 120; index++) entries.push(await log.write(record(index)))
    const lineBytes = Buffer.byteLength(`${JSON.stringify(entries[0])}\n`)

    const names = await readdir(directory)
    const sealed = names.filter((name) => name !== 'audit-current.log').sort()
    assert.isAbove(sealed.length, 1)
    const recovered = []
    for (const name of sealed) {
      const [, start, end] = name.match(SEALED) ?? []
      assert.exists(start, `unexpected sealed file name ${name}`)
      const { size } = await stat(join(directory, name))
      assert.isAtMost(size, 4096)
      assert.isAbove(size, 4096 - lineBytes)
      const lines = await readLines(join(directory, name))
      const parsed = lines.map((line) => JSON.parse(line))
      assert.equal(fileTimestamp(parsed[0].timestamp), start)
      assert.equal(fileTimestamp(parsed.at(-1).timestamp), end)
      recovered.push(...parsed)
    }
    const active = await readLines(log.activePath)
    recovered.push(...active.map((line) => JSON.parse(line)))
    assert.deepEqual(recovered, entries)
  })

  test('concurrent writes are serialized without losing entries', async ({ assert }) => {
    const log = new RotatingAuditLog({ directory, maxBytes: 4096, now: clock() })
    await Promise.all(Array.from({ length: 80 }, (_, index) => log.write(record(index))))
    let total = 0
    for (const name of await readdir(directory)) {
      const content = await readFile(join(directory, name), 'utf8')
      total += content.trim().split('\n').length
      if (name !== 'audit-current.log') assert.isAtMost(Buffer.byteLength(content), 4096)
    }
    assert.equal(total, 80)
  })

  test('a restarted writer continues the active file and keeps its original start date', async ({
    assert,
  }) => {
    const now = clock()
    await new RotatingAuditLog({ directory, maxBytes: 4096, now }).write(record(0))
    const restarted = new RotatingAuditLog({ directory, maxBytes: 4096, now })
    await restarted.write(record(1))
    const sealed = await restarted.flush()
    assert.equal(
      sealed,
      join(directory, 'audit-2026-01-01T00-00-01.000Z_2026-01-01T00-00-02.000Z.log')
    )
    assert.isNull(await restarted.flush())
  })

  test('never overwrites an existing sealed file with the same date range', async ({ assert }) => {
    const fixed = () => new Date('2026-01-01T00:00:00.000Z')
    const log = new RotatingAuditLog({ directory, maxBytes: 4096, now: fixed })
    const existing = join(directory, 'audit-2026-01-01T00-00-00.000Z_2026-01-01T00-00-00.000Z.log')
    await writeFile(existing, 'kept\n')
    await log.write(record(0))
    assert.equal(
      await log.flush(),
      join(directory, 'audit-2026-01-01T00-00-00.000Z_2026-01-01T00-00-00.000Z.1.log')
    )
    assert.equal(await readFile(existing, 'utf8'), 'kept\n')
  })

  test('rejects file sizes too small to hold entries', ({ assert }) => {
    assert.throws(() => new RotatingAuditLog({ directory, maxBytes: 100 }), /at least 4096/)
  })
})

test.group('Audit failure modes', () => {
  test('classifies failures into explicit, readable reasons', ({ assert }) => {
    const exception = (message: string, status: number) =>
      Object.assign(new Error(message), { status })
    assert.deepInclude(
      describeFailure({ code: 'E_VALIDATION_ERROR', messages: [{ field: 'label' }] }),
      { kind: 'invalid_input', reason: 'invalid input in field(s) label' }
    )
    assert.deepInclude(describeFailure(new InvalidInputError('Identificador invalido.')), {
      kind: 'invalid_input',
      reason: 'invalid input: Identificador invalido.',
    })
    assert.equal(describeFailure({ code: 'E_INVALID_CREDENTIALS' }).kind, 'invalid_credentials')
    assert.equal(describeFailure({ code: '23505' }).kind, 'conflict')
    assert.deepInclude(describeFailure(exception('Fracao nao encontrada.', 404)), {
      kind: 'not_found',
      reason: 'not found: Fracao nao encontrada.',
    })
    assert.equal(describeFailure(exception('Ja encerrada.', 409)).kind, 'conflict')
    assert.deepInclude(describeFailure(exception('A entrada nao pode ser futura.', 422)), {
      kind: 'rejected',
      reason: 'rejected by business rule: A entrada nao pode ser futura.',
    })
    assert.deepInclude(describeFailure(new Error('connect ECONNREFUSED'), true), {
      kind: 'error',
      reason: 'unexpected error Error: connect ECONNREFUSED',
      status: 500,
    })
  })
})
