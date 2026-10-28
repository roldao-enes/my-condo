export function databaseTls(mode: 'disable' | 'verify-full', certificate?: string) {
  if (mode === 'disable') {
    if (certificate) throw new Error('DB_SSL_CA requires DB_SSL_MODE=verify-full')
    return false
  }

  return {
    rejectUnauthorized: true,
    ...(certificate ? { ca: certificate.replace(/\\n/g, '\n') } : {}),
  }
}
