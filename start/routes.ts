/*
|--------------------------------------------------------------------------
| Routes file
|--------------------------------------------------------------------------
|
| The routes file is used for defining the HTTP routes.
|
*/

import router from '@adonisjs/core/services/router'
import { middleware } from '#start/kernel'
import { throttle } from '#start/limiter'
import db from '@adonisjs/lucid/services/db'
import type { HttpContext } from '@adonisjs/core/http'

const AuthController = () => import('#controllers/auth_controller')
const ResidencyController = () => import('#controllers/residency_controller')

router.get('/health/live', ({ response }) => {
  response.header('Cache-Control', 'no-store')
  return response.ok({ status: 'ok' })
})

async function readiness({ response }: HttpContext) {
  response.header('Cache-Control', 'no-store')
  try {
    await db.rawQuery('select 1').timeout(4000)
    return response.ok({ status: 'ok' })
  } catch {
    return response.serviceUnavailable({ status: 'unavailable' })
  }
}

router.get('/health', readiness)
router.get('/health/ready', readiness)

router.get('/login', [AuthController, 'show']).use(middleware.guest())
router.post('/login', [AuthController, 'store']).use(middleware.guest()).use(throttle)
router.post('/logout', [AuthController, 'destroy']).use(middleware.auth())

router
  .group(() => {
    router.get('/', [ResidencyController, 'index'])
    router.get('/units', [ResidencyController, 'index'])
    router.get('/residents', [ResidencyController, 'index'])
    router.post('/units', [ResidencyController, 'createUnit'])
    router.post('/residents', [ResidencyController, 'onboard'])
    router.post('/occupancies/:id/offboard', [ResidencyController, 'offboard'])
  })
  .use(middleware.auth())
