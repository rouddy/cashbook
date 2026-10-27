import { NavLink, Outlet } from 'react-router-dom'

import { logout } from '../api/client'
import { useRefData } from '../api/hooks'

const NAV = [
  { to: '/', label: '대시보드', icon: '◎', end: true },
  { to: '/transactions', label: '가계부', icon: '☰' },
  { to: '/entry', label: '결제 입력', icon: '＋' },
  { to: '/cards', label: '카드', icon: '▭' },
  { to: '/card-transactions', label: '카드별 내역', icon: '≡' },
  { to: '/accounts', label: '은행 잔고', icon: '₩' },
  { to: '/plan', label: '지출 계획', icon: '◫' },
  { to: '/settings', label: '설정', icon: '⚙' },
]

export default function Layout() {
  const { ref } = useRefData()
  return (
    <div className="app">
      <nav className="sidebar" aria-label="주 메뉴">
        <div className="brand">
          <span className="brand-mark">₩</span>
          가계부
        </div>
        {NAV.map((n) => (
          <NavLink key={n.to} to={n.to} end={n.end} className={({ isActive }) => `nav-link${isActive ? ' active' : ''}`}>
            <span className="nav-icon" aria-hidden>
              {n.icon}
            </span>
            {n.label}
          </NavLink>
        ))}
        <div className="sidebar-footer">
          {ref?.appInfo.authEnabled && (
            <button
              className="btn sm ghost"
              onClick={async () => {
                await logout()
                window.location.href = '/login'
              }}
            >
              로그아웃
            </button>
          )}
          <div>오늘 {ref?.appInfo.today ?? ''}</div>
          <div>v{ref?.appInfo.version ?? ''}</div>
        </div>
      </nav>
      <main className="main">
        <Outlet />
      </main>
    </div>
  )
}
