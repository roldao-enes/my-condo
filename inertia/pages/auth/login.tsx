import { Head, useForm } from '@inertiajs/react'
import { Building2, ArrowRight, LockKeyhole } from 'lucide-react'
import { Notices } from '../../components/shell'

export default function Login() {
  const form = useForm({ email: '', password: '' })
  return (
    <div className="login-page">
      <Head title="Entrar" />
      <div className="login-main">
        <div className="brand">
          <span className="brand-mark">
            <Building2 size={26} />
          </span>
          <span>
            meu<span className="brand-light">condomínio</span>
          </span>
        </div>
        <section className="login-form">
          <span className="eyebrow">ADMINISTRAÇÃO</span>
          <h1>Bem-vindo a casa.</h1>
          <p className="muted">Entre no seu condomínio.</p>
          <Notices />
          <form
            onSubmit={(event) => {
              event.preventDefault()
              form.post('/login', { onFinish: () => form.reset('password') })
            }}
          >
            <label>
              Email
              <input
                type="email"
                name="email"
                autoComplete="username"
                autoFocus
                required
                value={form.data.email}
                onChange={(event) => form.setData('email', event.target.value)}
              />
            </label>
            <label>
              Palavra-passe
              <input
                type="password"
                name="password"
                autoComplete="current-password"
                required
                value={form.data.password}
                onChange={(event) => form.setData('password', event.target.value)}
              />
            </label>
            <button className="button primary login-submit" disabled={form.processing}>
              {form.processing ? 'A entrar...' : 'Entrar'}
              <ArrowRight size={18} />
            </button>
          </form>
          <div className="login-private">
            <LockKeyhole size={15} />
            Acesso reservado à administração
          </div>
        </section>
        <div className="login-footer">Portugal · EUR</div>
      </div>
      <div className="login-photo" role="img" aria-label="Fachada de um edificio residencial">
        <div className="photo-caption">
          <span>O NOSSO ESPACO COMUM</span>
          <p>
            Uma casa.
            <br />
            Uma comunidade.
          </p>
        </div>
      </div>
    </div>
  )
}
