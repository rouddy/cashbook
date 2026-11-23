import { useQuery } from '@tanstack/react-query'
import { NavLink, Outlet } from 'react-router-dom'

import { fetchSession, signOut } from '../api/client'
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
  const session = useQuery({ queryKey: ['session'], queryFn: fetchSession, staleTime: Infinity })
  const user = session.data?.authRequired ? session.data.user : null
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
          {user && (
            <div className="sidebar-user" title={user.email ?? undefined}>
              {user.picture && <img src={user.picture} alt="" referrerPolicy="no-referrer" />}
              <span>{user.name ?? user.email}</span>
            </div>
          )}
          {user && (
            <button className="btn sm ghost" onClick={signOut}>
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
