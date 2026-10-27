import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { Navigate, Route, Routes } from 'react-router-dom'

import { UNAUTHORIZED_EVENT, fetchSession } from './api/client'
import { ErrorBox, Loading } from './components/Common'
import Layout from './components/Layout'
import Accounts from './pages/Accounts'
import CardDetail from './pages/CardDetail'
import Cards from './pages/Cards'
import CardTransactions from './pages/CardTransactions'
import Dashboard from './pages/Dashboard'
import Entry from './pages/Entry'
import Login from './pages/Login'
import Plan from './pages/Plan'
import Settings from './pages/Settings'
import Transactions from './pages/Transactions'

export default function App() {
  const qc = useQueryClient()
  const session = useQuery({ queryKey: ['session'], queryFn: fetchSession, staleTime: Infinity })

  useEffect(() => {
    const onUnauthorized = () => qc.invalidateQueries({ queryKey: ['session'] })
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized)
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized)
  }, [qc])

  if (session.isLoading) return <Loading />
  if (session.error) return <div style={{ padding: 24 }}><ErrorBox error={session.error} /></div>
  const s = session.data!
  if (s.authRequired && !s.authenticated) {
    return (
      <Routes>
        <Route path="*" element={<Login />} />
      </Routes>
    )
  }

  return (
    <Routes>
      <Route path="/login" element={<Navigate to="/" replace />} />
      <Route element={<Layout />}>
        <Route index element={<Dashboard />} />
        <Route path="transactions" element={<Transactions />} />
        <Route path="entry" element={<Entry />} />
        <Route path="cards" element={<Cards />} />
        <Route path="cards/:id" element={<CardDetail />} />
        <Route path="card-transactions" element={<CardTransactions />} />
        <Route path="accounts" element={<Accounts />} />
        <Route path="plan" element={<Plan />} />
        <Route path="settings" element={<Settings />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  )
}
