import { Link, router, usePage } from '@inertiajs/react'
import {
  Building2,
  LayoutDashboard,
  DoorOpen,
  Users,
  LogOut,
  ArrowUpRight,
  CheckCircle2,
  AlertCircle,
} from 'lucide-react'
import type { PropsWithChildren } from 'react'
import { initials, type Shared } from '../lib/types'

export function Notices() {
  const { flash, errors } = usePage<Shared>().props
  const error = flash?.error || Object.values(errors || {}).join(' ')
  return (
    <>
      {flash?.success && (
        <div className="notice success" role="status">
          <CheckCircle2 size={18} />
          {flash.success}
        </div>
      )}
      {error && (
        <div className="notice error" role="alert">
          <AlertCircle size={18} />
          {error}
        </div>
      )}
    </>
  )
}

export default function Shell({ children, view }: PropsWithChildren<{ view: string }>) {
  const { user } = usePage<Shared>().props
  const links = [
    { id: 'overview', href: '/', label: 'Visão geral', icon: LayoutDashboard },
    { id: 'units', href: '/units', label: 'Frações', icon: DoorOpen },
    { id: 'residents', href: '/residents', label: 'Residentes', icon: Users },
  ]
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <Link href="/" className="brand">
          <span className="brand-mark">
            <Building2 size={24} />
          </span>
          <span>
            meu<span className="brand-light">condomínio</span>
            <small>ESPACO DE GESTAO</small>
          </span>
        </Link>
        <div className="workspace-label">
          <span className="workspace-dot" />O meu condomínio<small>Administração</small>
        </div>
        <div className="nav-caption">PRINCIPAL</div>
        <nav aria-label="Navegacao principal">
          {links.map(({ id, href, label, icon: Icon }) => (
            <Link
              key={id}
              href={href}
              className={`nav-link ${view === id ? 'active' : ''}`}
              aria-current={view === id ? 'page' : undefined}
            >
              <Icon size={19} />
              {label}
              {view === id && <span className="active-dot" />}
            </Link>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="locale-label">
            <span className="country-mark">PT</span>
            <span>
              Portugal<small>EUR · Europe/Lisbon</small>
            </span>
          </div>
          <div className="account">
            <span className="avatar">{initials(user?.name || 'Admin')}</span>
            <span className="account-name">
              {user?.name || 'Administrador'}
              <small>Administrador</small>
            </span>
            <button
              className="icon-button"
              title="Terminar sessao"
              aria-label="Terminar sessao"
              onClick={() => router.post('/logout')}
            >
              <LogOut size={18} />
            </button>
          </div>
        </div>
      </aside>
      <div className="main-wrap">
        <header className="topbar">
          <span>
            O meu condomínio <span className="breadcrumb-divider">/</span>{' '}
            <strong>{links.find((link) => link.id === view)?.label}</strong>
          </span>
          <span className="private-label">
            <span className="workspace-dot" />
            Espaco privado <ArrowUpRight size={14} />
          </span>
          <button
            className="icon-button mobile-logout"
            title="Terminar sessao"
            aria-label="Terminar sessao"
            onClick={() => router.post('/logout')}
          >
            <LogOut size={18} />
          </button>
        </header>
        <main id="main-content">
          <Notices />
          {children}
        </main>
        <footer className="page-footer">
          <span>Meu Condomínio</span>
          <span>Portugal · EUR</span>
        </footer>
      </div>
    </div>
  )
}
