// 장부 이체: 한 거래가 보내는 장부(ledgerId)에서 받는 장부(transferLedgerId)로 금액을 옮긴다.
// 보내는 장부에서는 나가는 돈, 받는 장부에서는 들어오는 돈이고 소비·수입 합계에는 넣지 않는다 (서버 summary::classify).

import type { RefMaps } from '../api/hooks'
import type { Transaction } from '../api/types'

type TransferLike = Pick<Transaction, 'ledgerId' | 'transferLedgerId'>

/** 'in' = 보고 있는 장부가 받은 이체, 'out' = 보낸 이체, 'between' = 전체 장부 보기의 이체, null = 이체 아님 */
export function transferDirection(tx: TransferLike, viewLedgerId: number | null | undefined): 'in' | 'out' | 'between' | null {
  if (tx.transferLedgerId == null) return null
  if (viewLedgerId != null && viewLedgerId === tx.transferLedgerId) return 'in'
  if (viewLedgerId != null && viewLedgerId === tx.ledgerId) return 'out'
  return 'between'
}

/** 항목 자리에 보여줄 이체 표시: '→ 용돈', '← 가계부', '가계부 → 용돈' */
export function transferLabel(tx: TransferLike, refMaps: RefMaps, viewLedgerId: number | null | undefined): string | null {
  const dir = transferDirection(tx, viewLedgerId)
  if (!dir) return null
  const from = refMaps.ledgerById.get(tx.ledgerId)?.name ?? '?'
  const to = refMaps.ledgerById.get(tx.transferLedgerId ?? -1)?.name ?? '?'
  return dir === 'in' ? `← ${from}` : dir === 'out' ? `→ ${to}` : `${from} → ${to}`
}

/** 항목 선택 목록에서 장부 이체를 뜻하는 값 (`t:<받는 장부 id>`) */
export const transferOption = (ledgerId: number) => `t:${ledgerId}`

export function parseTransferOption(value: string): number | null {
  return value.startsWith('t:') ? Number(value.slice(2)) : null
}

/** `ledgerId` 장부에서 보낼 수 있는 장부 이체 선택지 */
export function transferOptions(refMaps: RefMaps, ledgerId: number | null | undefined) {
  return refMaps.ledgers.filter((l) => l.id !== ledgerId).map((l) => ({ value: transferOption(l.id), label: `→ ${l.name} (장부 이체)` }))
}
