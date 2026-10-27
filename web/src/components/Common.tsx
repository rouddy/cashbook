import type { ReactNode } from 'react'

import { errorMessage } from '../api/client'
import { won } from '../lib/format'

export function Loading({ label = '불러오는 중…' }: { label?: string }) {
  return <div className="empty">{label}</div>
}

export function ErrorBox({ error }: { error: unknown }) {
  return <div className="error-box">{errorMessage(error)}</div>
}

export function Panel({
  title,
  sub,
  actions,
  children,
  flush,
  className,
}: {
  title?: ReactNode
  sub?: ReactNode
  actions?: ReactNode
  children: ReactNode
  flush?: boolean
  className?: string
}) {
  return (
    <section className={`panel ${className ?? ''}`}>
      {(title || actions) && (
        <div className="panel-head">
          <div>
            {title && <h2>{title}</h2>}
            {sub && <div className="panel-sub">{sub}</div>}
          </div>
          <span className="spacer" />
          {actions}
        </div>
      )}
      <div className={`panel-body ${flush ? 'flush' : ''}`}>{children}</div>
    </section>
  )
}

/** 지출 금액: 음수(환불)는 −로, 수입은 +로 표시 */
export function Amount({ value, income, strong }: { value: number; income?: boolean; strong?: boolean }) {
  const text = income ? `+${won(value)}` : won(value)
  const cls = value < 0 || income ? 'good' : ''
  return <span className={`num ${cls}`} style={strong ? { fontWeight: 650 } : undefined}>{text}</span>
}

/** 증감: 지출은 늘면 나쁨(▲ 빨강), 줄면 좋음(▼ 초록) */
export function Delta({ value, upIsGood = false, suffix = '' }: { value: number; upIsGood?: boolean; suffix?: string }) {
  if (!Number.isFinite(value) || value === 0) return <span className="text-2">변화 없음</span>
  const up = value > 0
  const good = up === upIsGood
  return (
    <span className={good ? 'good' : 'bad'}>
      {up ? '▲' : '▼'} {won(Math.abs(value))}
      {suffix}
    </span>
  )
}

/** goal=true면 목표형(넘으면 달성), 아니면 한도형(넘으면 초과 경고) */
export function Meter({ value, max, tick, label, goal }: { value: number; max: number; tick?: number; label: string; goal?: boolean }) {
  const ratio = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0
  const over = !goal && max > 0 && value > max
  const tickRatio = tick != null && max > 0 ? Math.min(1, tick / max) : null
  return (
    <div
      className={`meter ${over ? 'over' : ''}`}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
    >
      <span style={{ width: `${ratio * 100}%` }} />
      {tickRatio != null && <i className="tick" style={{ left: `calc(${tickRatio * 100}% - 1px)` }} />}
    </div>
  )
}

export function StatTile({ label, value, delta, children }: { label: string; value: ReactNode; delta?: ReactNode; children?: ReactNode }) {
  return (
    <div className="panel stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {delta && <div className="stat-delta">{delta}</div>}
      {children}
    </div>
  )
}
