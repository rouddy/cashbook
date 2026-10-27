import { useMemo, useState, type KeyboardEvent, type PointerEvent } from 'react'

import { compactWon, won } from '../../lib/format'
import { niceScale, useWidth } from './useWidth'

export interface LineSeries {
  key: string
  name: string
  /** null이면 그 지점은 그리지 않는다 (예: 오늘 이후 확정 누적) */
  values: (number | null)[]
  color: string
  opacity?: number
  /** 아래 영역을 옅게 칠한다 (주 시리즈 하나에만) */
  area?: boolean
  /** 끝에 값 라벨을 붙인다 */
  endLabel?: boolean
}

interface Props {
  labels: string[]
  series: LineSeries[]
  height?: number
  /** 툴팁 제목 */
  titleFor?: (i: number) => string
  /** x축에 표시할 눈금 인덱스 */
  xTicks?: number[]
  ariaLabel: string
}

const M = { top: 12, right: 64, bottom: 26, left: 52 }

/**
 * 선 차트: 2px 선, 1px 실선 격자, 크로스헤어 + 전체 시리즈 툴팁.
 * 범례는 항상 표시하고 끝값 라벨은 겹치지 않을 때만 단다.
 */
export default function LineChart({ labels, series, height = 240, titleFor, xTicks, ariaLabel }: Props) {
  const [ref, width] = useWidth<HTMLDivElement>()
  const [hover, setHover] = useState<number | null>(null)
  const n = labels.length
  const plotW = Math.max(10, width - M.left - M.right)
  const plotH = height - M.top - M.bottom

  const maxValue = useMemo(
    () => Math.max(0, ...series.flatMap((s) => s.values.filter((v): v is number => v != null))),
    [series],
  )
  const { max, step } = niceScale(maxValue)
  const x = (i: number) => M.left + (n <= 1 ? plotW / 2 : (i / (n - 1)) * plotW)
  const y = (v: number) => M.top + plotH - (Math.max(0, v) / max) * plotH

  const paths = series.map((s) => {
    let d = ''
    let started = false
    s.values.forEach((v, i) => {
      if (v == null) {
        started = false
        return
      }
      d += `${started ? 'L' : 'M'}${x(i).toFixed(1)},${y(v).toFixed(1)}`
      started = true
    })
    const lastIdx = s.values.reduce<number>((acc, v, i) => (v != null ? i : acc), -1)
    const firstIdx = s.values.findIndex((v) => v != null)
    const area =
      s.area && lastIdx > firstIdx
        ? `${d}L${x(lastIdx).toFixed(1)},${y(0).toFixed(1)}L${x(firstIdx).toFixed(1)},${y(0).toFixed(1)}Z`
        : null
    return { s, d, area, lastIdx }
  })

  // 끝값 라벨: 서로 14px 이내로 겹치면 뒤쪽(덜 중요한) 라벨은 생략하고 범례/툴팁에 맡긴다.
  const endLabels: { key: string; yPos: number; text: string; xPos: number }[] = []
  for (const p of paths) {
    if (!p.s.endLabel || p.lastIdx < 0) continue
    const v = p.s.values[p.lastIdx] as number
    const yPos = y(v)
    if (endLabels.some((l) => Math.abs(l.yPos - yPos) < 14)) continue
    endLabels.push({ key: p.s.key, yPos, text: compactWon(v), xPos: x(p.lastIdx) })
  }

  const ticks: number[] = []
  for (let v = 0; v <= max + 1e-9; v += step) ticks.push(v)
  const xt = xTicks ?? [0, Math.floor((n - 1) / 2), n - 1]

  const onMove = (e: PointerEvent<SVGRectElement>) => {
    const rect = (e.currentTarget as SVGRectElement).getBoundingClientRect()
    const px = e.clientX - rect.left
    const i = Math.round((px / rect.width) * (n - 1))
    setHover(Math.min(n - 1, Math.max(0, i)))
  }
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'ArrowRight') setHover((h) => Math.min(n - 1, (h ?? -1) + 1))
    else if (e.key === 'ArrowLeft') setHover((h) => Math.max(0, (h ?? n) - 1))
    else if (e.key === 'Escape') setHover(null)
    else return
    e.preventDefault()
  }

  const tooltipLeft = hover != null ? Math.min(Math.max(x(hover) + 12, 0), width - 190) : 0

  return (
    <div>
      <div className="legend" style={{ marginBottom: 8 }}>
        {series.map((s) => (
          <span key={s.key}>
            <i className="line-key" style={{ color: s.color, opacity: s.opacity ?? 1 }} />
            {s.name}
          </span>
        ))}
      </div>
      <div
        ref={ref}
        className="chart"
        tabIndex={0}
        role="img"
        aria-label={ariaLabel}
        onKeyDown={onKey}
        onBlur={() => setHover(null)}
      >
        <svg height={height} viewBox={`0 0 ${width} ${height}`}>
          {ticks.map((t) => (
            <g key={t}>
              <line className={t === 0 ? 'axis-line' : 'grid-line'} x1={M.left} x2={M.left + plotW} y1={y(t)} y2={y(t)} />
              <text className="tick-label" x={M.left - 8} y={y(t) + 4} textAnchor="end">
                {compactWon(t)}
              </text>
            </g>
          ))}
          {xt.map((i) => (
            <text key={i} className="tick-label" x={x(i)} y={height - 6} textAnchor="middle">
              {labels[i]}
            </text>
          ))}
          {paths.map(({ s, area }) =>
            area ? <path key={`${s.key}-area`} d={area} fill={s.color} opacity={0.1} /> : null,
          )}
          {paths.map(({ s, d }) => (
            <path
              key={s.key}
              d={d}
              fill="none"
              stroke={s.color}
              strokeOpacity={s.opacity ?? 1}
              strokeWidth={2}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ))}
          {paths.map(({ s, lastIdx }) =>
            lastIdx >= 0 && s.endLabel ? (
              <circle
                key={`${s.key}-end`}
                cx={x(lastIdx)}
                cy={y(s.values[lastIdx] as number)}
                r={4}
                fill={s.color}
                stroke="var(--surface-chart)"
                strokeWidth={2}
              />
            ) : null,
          )}
          {endLabels.map((l) => (
            <text key={l.key} className="end-label" x={l.xPos + 8} y={l.yPos + 4}>
              {l.text}
            </text>
          ))}
          {hover != null && (
            <g pointerEvents="none">
              <line className="axis-line" x1={x(hover)} x2={x(hover)} y1={M.top} y2={M.top + plotH} />
              {series.map((s) => {
                const v = s.values[hover]
                return v == null ? null : (
                  <circle
                    key={s.key}
                    cx={x(hover)}
                    cy={y(v)}
                    r={4}
                    fill={s.color}
                    fillOpacity={s.opacity ?? 1}
                    stroke="var(--surface-chart)"
                    strokeWidth={2}
                  />
                )
              })}
            </g>
          )}
          <rect
            x={M.left}
            y={M.top}
            width={plotW}
            height={plotH}
            fill="transparent"
            onPointerMove={onMove}
            onPointerLeave={() => setHover(null)}
          />
        </svg>
        {hover != null && (
          <div className="chart-tooltip" style={{ left: tooltipLeft, top: 8 }}>
            <div className="tt-title">{titleFor ? titleFor(hover) : labels[hover]}</div>
            {series.map((s) => {
              const v = s.values[hover]
              return (
                <div key={s.key} className="tt-row">
                  <i className="line-key" style={{ color: s.color, opacity: s.opacity ?? 1 }} />
                  <span className="tt-value">{v == null ? '-' : `${won(v)}원`}</span>
                  <span className="tt-name">{s.name}</span>
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
