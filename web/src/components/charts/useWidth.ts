import { useEffect, useRef, useState } from 'react'

/** 컨테이너 폭을 따라가는 반응형 차트용 */
export function useWidth<T extends HTMLElement>(fallback = 600) {
  const ref = useRef<T>(null)
  const [width, setWidth] = useState(fallback)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width
      if (w) setWidth((prev) => (Math.abs(w - prev) > 1 ? w : prev))
    })
    ro.observe(el)
    setWidth(el.getBoundingClientRect().width || fallback)
    return () => ro.disconnect()
  }, [fallback])
  return [ref, width] as const
}

/** 0에서 시작하는 보기 좋은 최대값과 눈금 */
export function niceScale(max: number, ticks = 4): { max: number; step: number } {
  if (!Number.isFinite(max) || max <= 0) return { max: 1, step: 1 }
  const rough = max / ticks
  const mag = 10 ** Math.floor(Math.log10(rough))
  const norm = rough / mag
  const nice = norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 2.5 ? 2.5 : norm <= 5 ? 5 : 10
  const step = nice * mag
  return { max: Math.ceil(max / step) * step, step }
}
