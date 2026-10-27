import { useCallback } from 'react'
import { useSearchParams } from 'react-router-dom'

/** "YYYY-MM" 문자열로 월을 다룬다. */

export function localToday(): string {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

export function monthOf(date: string): string {
  return date.slice(0, 7)
}

export function currentMonth(): string {
  return monthOf(localToday())
}

export function isMonth(s: string | null | undefined): s is string {
  return !!s && /^\d{4}-(0[1-9]|1[0-2])$/.test(s)
}

export function addMonths(month: string, n: number): string {
  const [y, m] = month.split('-').map(Number)
  const idx = y * 12 + (m - 1) + n
  const ny = Math.floor(idx / 12)
  const nm = (idx % 12) + 1
  return `${ny}-${String(nm).padStart(2, '0')}`
}

export function monthLabel(month: string): string {
  const [y, m] = month.split('-').map(Number)
  return `${y}년 ${m}월`
}

export function shortMonthLabel(month: string): string {
  const [y, m] = month.split('-').map(Number)
  return `${String(y).slice(2)}.${m}`
}

export function daysInMonth(month: string): number {
  const [y, m] = month.split('-').map(Number)
  return new Date(y, m, 0).getDate()
}

export function firstDay(month: string): string {
  return `${month}-01`
}

export function lastDay(month: string): string {
  return `${month}-${String(daysInMonth(month)).padStart(2, '0')}`
}

/** 이 달에 새 거래를 넣을 때 기본 날짜: 이번 달이면 오늘, 아니면 그 달 1일 */
export function defaultDateFor(month: string): string {
  const today = localToday()
  return monthOf(today) === month ? today : firstDay(month)
}

/** URL의 ?month= 값과 동기화되는 월 상태 */
export function useMonthParam(): [string, (m: string) => void] {
  const [params, setParams] = useSearchParams()
  const raw = params.get('month')
  const month = isMonth(raw) ? raw : currentMonth()
  const setMonth = useCallback(
    (m: string) => {
      // 한 핸들러에서 여러 파라미터를 바꿔도 덮어쓰지 않도록 항상 현재 주소에서 시작한다
      const next = new URLSearchParams(window.location.search)
      next.set('month', m)
      setParams(next, { replace: true })
    },
    [setParams],
  )
  return [month, setMonth]
}

/** URL 쿼리 파라미터 하나를 상태처럼 쓴다 */
export function useQueryParam(name: string, fallback: string): [string, (v: string) => void] {
  const [params, setParams] = useSearchParams()
  const value = params.get(name) ?? fallback
  const set = useCallback(
    (v: string) => {
      const next = new URLSearchParams(window.location.search)
      if (v === fallback) next.delete(name)
      else next.set(name, v)
      setParams(next, { replace: true })
    },
    [setParams, name, fallback],
  )
  return [value, set]
}

/** "YYYY-MM-DD" + n일 */
export function addDays(date: string, days: number): string {
  const [y, m, d] = date.split('-').map(Number)
  const dt = new Date(y, m - 1, d + days)
  return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, '0')}-${String(dt.getDate()).padStart(2, '0')}`
}
