import { useState } from 'react'

import type { Transaction } from '../api/types'

/**
 * 거래 그리드의 선택 모드 상태. `grid`를 `TransactionGrid`의 `selection`에 넘긴다.
 * `chosen`은 지금 목록(`rows`)에 보이는 체크한 거래만 — 달·필터를 바꿔 사라진 거래는 고르지 않는다.
 */
export function useTxSelection(rows: Transaction[] | undefined) {
  const [selecting, setSelecting] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(() => new Set())
  const chosen = rows?.filter((t) => selected.has(t.id)) ?? []
  const stop = () => {
    setSelecting(false)
    setSelected(new Set())
  }
  return {
    selecting,
    chosen,
    start: () => setSelecting(true),
    stop,
    grid: selecting ? { selected, onChange: setSelected } : undefined,
  }
}
