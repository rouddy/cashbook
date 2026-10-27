import { useState, type KeyboardEvent } from 'react'

import { compactWon, won } from '../../lib/format'
import { niceScale, useWidth } from './useWidth'

interface Props {
  labels: string[]
  values: number[]
  /** 강조할 막대 (예: 선택한 달). 나머지는 맥락 회색 */
  highlight?: number
  /** 기준선 (예: 평균) */
  reference?: { value: number; label: string }
  titleFor?: (i: number) => string
  detailFor?: (i: number) => string | null
  height?: number
  ariaLabel: string
  onSelect?: (i: number) => void
}

const M = { top: 22, right: 12, bottom: 24, left: 52 }

/** 세로 막대: 최대 24px 두께, 위쪽 4px 둥근 끝, 막대별 툴팁 */
export default function ColumnChart({ labels, values, highlight, reference, titleFor, detailFor, height = 200, ariaLabel, onSelect }: Props) {
  const [ref, width] = useWidth<HTMLDivElement>()
  const [hover, setHover] = useState<number | null>(null)
  const n = values.length
  const plotW = Math.max(10, width - M.left - M.right)
  const plotH = height - M.top - M.bottom
  const { max, step } = niceScale(Math.max(1, ...values, reference?.value ?? 0))
  const band = plotW / Math.max(1, n)
  const barW = Math.min(24, Math.max(4, band - 6))
  const y = (v: number) => M.top + plotH - (Math.max(0, v) / max) * plotH
  const cx = (i: number) => M.left + band * i + band / 2

  const ticks: number[] = []
  for (let v = 0; v <= max + 1e-9; v += step) ticks.push(v)
  const labelEvery = Math.ceil(n / Math.max(1, Math.floor(plotW / 44)))

  const barPath = (i: number) => {
    const v = Math.max(0, values[i])
    const top = y(v)
    const base = y(0)
    const h = base - top
    const left = cx(i) - barW / 2
    const r = Math.min(4, h, barW / 2)
    if (h <= 0.5) return ''
    return `M${left},${base}V${top + r}Q${left},${top} ${left + r},${top}H${left + barW - r}Q${left + barW},${top} ${left + barW},${top + r}V${base}Z`
  }

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'ArrowRight') setHover((h) => Math.min(n - 1, (h ?? -1) + 1))
    else if (e.key === 'ArrowLeft') setHover((h) => Math.max(0, (h ?? n) - 1))
    else if (e.key === 'Enter' && hover != null) onSelect?.(hover)
    else return
    e.preventDefault()
  }

  return (
    <div ref={ref} className="chart" tabIndex={0} role="img" aria-label={ariaLabel} onKeyDown={onKey} onBlur={() => setHover(null)}>
      <svg height={height} viewBox={`0 0 ${width} ${height}`}>
        {ticks.map((t) => (
          <g key={t}>
            <line className={t === 0 ? 'axis-line' : 'grid-line'} x1={M.left} x2={M.left + plotW} y1={y(t)} y2={y(t)} />
            <text className="tick-label" x={M.left - 8} y={y(t) + 4} textAnchor="end">
              {compactWon(t)}
            </text>
          </g>
        ))}
        {values.map((_, i) => (
          <path
            key={i}
            d={barPath(i)}
            fill={i === highlight ? 'var(--accent)' : 'var(--series-context)'}
            fillOpacity={i === highlight ? 1 : hover === i ? 0.75 : 0.45}
          />
        ))}
        {reference && reference.value > 0 && (
          <g>
            <line x1={M.left} x2={M.left + plotW} y1={y(reference.value)} y2={y(reference.value)} stroke="var(--text-2)" strokeWidth={1} />
            <text className="tick-label" x={M.left + 4} y={y(reference.value) - 4} textAnchor="start">
              {reference.label} {compactWon(reference.value)}
            </text>
          </g>
        )}
        {highlight != null && values[highlight] > 0 && (
          <text className="end-label" x={cx(highlight)} y={y(values[highlight]) - 6} textAnchor="middle">
            {compactWon(values[highlight])}
          </text>
        )}
        {labels.map((l, i) =>
          i % labelEvery === 0 || i === highlight ? (
            <text key={i} className="tick-label" x={cx(i)} y={height - 6} textAnchor="middle" fontWeight={i === highlight ? 700 : 400}>
              {l}
            </text>
          ) : null,
        )}
        {values.map((_, i) => (
          <rect
            key={`hit-${i}`}
            x={cx(i) - band / 2}
            y={M.top}
            width={band}
            height={plotH}
            fill="transparent"
            style={{ cursor: onSelect ? 'pointer' : 'default' }}
            onPointerEnter={() => setHover(i)}
            onPointerLeave={() => setHover(null)}
            onClick={() => onSelect?.(i)}
          />
        ))}
      </svg>
      {hover != null && (
        <div className="chart-tooltip" style={{ left: Math.min(Math.max(cx(hover) - 80, 0), width - 180), top: 0 }}>
          <div className="tt-title">{titleFor ? titleFor(hover) : labels[hover]}</div>
          <div className="tt-row">
            <span className="tt-value">{won(values[hover])}원</span>
          </div>
          {detailFor?.(hover) && <div className="tt-name">{detailFor(hover)}</div>}
        </div>
      )}
    </div>
  )
}
