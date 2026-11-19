/** Minutes to wait after the Nth failed attempt (index 0 = first failure). */
const BACKOFF_MINUTES = [5, 30, 180, 720] as const

export const MAX_ATTEMPTS = BACKOFF_MINUTES.length + 1

/** How long a claimed job is leased before another run may pick it up (crash recovery). */
export const CLAIM_LEASE_MINUTES = 10

/**
 * Given the number of attempts made so far (including the one that just failed),
 * returns the delay before the next attempt, or `null` when the job should be given up.
 */
export function retryDelayMinutes(attempts: number): number | null {
  if (!Number.isInteger(attempts) || attempts < 1) {
    throw new RangeError('attempts must be a positive integer')
  }
  return attempts >= MAX_ATTEMPTS ? null : BACKOFF_MINUTES[attempts - 1]
}
