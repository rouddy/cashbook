import type { ReactNode } from 'react'

import { errorMessage } from '../api/client'
import type { CardMonthReport } from '../api/types'
import { plannedPerformance } from '../lib/benefits'
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

/** goal=true면 목표형(넘으면 달성), 아니면 한도형(넘으면 초과 경고). planned는 value 뒤에 다른 색으로 쌓는 예정분. */
export function Meter({
  value,
  planned = 0,
  max,
  ticks,
  label,
  title,
  valueText,
  goal,
}: {
  value: number
  planned?: number
  max: number
  /** 막대 위 구분선 위치 (값 기준) */
  ticks?: number[]
  label: string
  title?: string
  valueText?: string
  goal?: boolean
}) {
  const clamp = (v: number) => (max > 0 ? Math.min(1, Math.max(0, v / max)) : 0)
  const ratio = clamp(value)
  const plannedRatio = clamp(value + Math.max(0, planned)) - ratio
  const over = !goal && max > 0 && value > max
  const tickRatios = max > 0 ? (ticks ?? []).filter((t) => t >= 0).map((t) => Math.min(1, t / max)) : []
  // 확정분이 있으면 그 끝에서 2px 띄운다
  const gap = ratio > 0 ? 2 : 0
  return (
    <div
      className={`meter ${over ? 'over' : ''}`}
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={valueText}
      title={title}
    >
      <span style={{ width: `${ratio * 100}%` }} />
      {plannedRatio > 0 && (
        <span
          className="planned"
          style={{ left: `calc(${ratio * 100}% + ${gap}px)`, width: `max(0px, calc(${plannedRatio * 100}% - ${gap}px))` }}
        />
      )}
      {tickRatios.map((t) => (
        <i key={t} className="tick" style={{ left: `calc(${t * 100}% - 1px)` }} />
      ))}
    </div>
  )
}

/** 실적 막대 예정분의 색 견본 (예정분 글자 앞에 둔다) */
export function PlannedSwatch() {
  return <i className="swatch planned key" aria-hidden />
}

/** 전월실적 막대: 끝은 마지막 단계, 중간 단계마다 구분선. 확정 실적은 진한 파랑, 예정분은 그 뒤에 밝은 파랑. 단계가 없으면 그리지 않는다. */
export function PerformanceMeter({ report: r, label }: { report: CardMonthReport; label: string }) {
  const steps = r.performanceSteps
  const max = steps[steps.length - 1]
  if (max == null) return null
  const planned = Math.max(0, plannedPerformance(r))
  const amounts = `확정 ${won(r.performance)}원` + (planned > 0 ? ` · 예정 +${won(planned)}원 (예정 포함 ${won(r.projectedPerformance)}원)` : '')
  return (
    <Meter
      goal
      value={r.performance}
      planned={planned}
      max={max}
      ticks={steps.slice(0, -1)}
      label={label}
      valueText={amounts}
      title={steps.length > 1 ? `${amounts}\n실적 단계 ${steps.map((s) => won(s)).join(' · ')}` : amounts}
    />
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
