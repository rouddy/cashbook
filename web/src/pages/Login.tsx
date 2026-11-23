import { useLocation, useSearchParams } from 'react-router-dom'

import { googleLoginUrl } from '../api/client'

/** Google 계정으로 로그인. 처음 로그인하면 그 계정의 장부가 새로 생긴다 */
export default function Login() {
  const location = useLocation()
  const [params] = useSearchParams()
  const error = params.get('error')
  // 로그인 화면 자체가 아니면 지금 보던 화면으로 돌아온다
  const next = location.pathname.startsWith('/login') ? '/' : `${location.pathname}${location.search}`

  return (
    <div style={{ minHeight: '100vh', display: 'grid', placeItems: 'center', padding: 16 }}>
      <div className="panel" style={{ width: 'min(360px, 100%)', padding: 24 }}>
        <div className="brand" style={{ padding: '0 0 16px' }}>
          <span className="brand-mark">₩</span>
          가계부
        </div>
        <p className="muted" style={{ margin: '0 0 16px', fontSize: 13 }}>
          Google 계정으로 로그인합니다. 처음 로그인하면 그 계정의 장부가 새로 만들어집니다.
        </p>
        {error && (
          <div className="error-box" role="alert" style={{ marginBottom: 12 }}>
            {error}
          </div>
        )}
        <a className="btn primary" style={{ width: '100%', textDecoration: 'none' }} href={googleLoginUrl(next)}>
          Google로 로그인
        </a>
      </div>
    </div>
  )
}
