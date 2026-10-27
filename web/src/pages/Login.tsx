import { useQueryClient } from '@tanstack/react-query'
import { useState, type FormEvent } from 'react'

import { errorMessage, login } from '../api/client'

export default function Login() {
  const qc = useQueryClient()
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await login(password)
      await qc.invalidateQueries()
    } catch (err) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div style={{ minHeight: '100vh', display: 'grid', placeItems: 'center', padding: 16 }}>
      <form className="panel" style={{ width: 'min(360px, 100%)', padding: 24 }} onSubmit={submit}>
        <div className="brand" style={{ padding: '0 0 16px' }}>
          <span className="brand-mark">₩</span>
          가계부
        </div>
        <label className="field">
          비밀번호
          <input
            className="input"
            type="password"
            autoFocus
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </label>
        {error && (
          <div className="error-box" style={{ marginTop: 12 }}>
            {error}
          </div>
        )}
        <button className="btn primary" style={{ width: '100%', marginTop: 16 }} disabled={busy || !password}>
          {busy ? '확인 중…' : '로그인'}
        </button>
      </form>
    </div>
  )
}
