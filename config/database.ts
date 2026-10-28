import env from '#start/env'
import { defineConfig } from '@adonisjs/lucid'
import { databaseTls } from '../app/deployment/database_tls.js'

const dbConfig = defineConfig({
  connection: 'postgres',
  connections: {
    postgres: {
      client: 'pg',
      connection: {
        host: env.get('DB_HOST'),
        port: env.get('DB_PORT'),
        user: env.get('DB_USER'),
        password: env.get('DB_PASSWORD'),
        database: env.get('DB_DATABASE'),
        ssl: databaseTls(env.get('DB_SSL_MODE', 'disable'), env.get('DB_SSL_CA')),
      },
      pool: {
        min: 0,
        max: env.get('DB_POOL_MAX', 10),
        createTimeoutMillis: 5000,
        acquireTimeoutMillis: 5000,
      },
      migrations: {
        naturalSort: true,
        paths: ['database/migrations'],
      },
    },
  },
})

export default dbConfig
