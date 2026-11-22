// 가족카드: 본인 카드(`parentId`)와 전월실적·혜택을 함께 쓰는 카드.
// 종류·결제 계좌·정산 일정은 서버가 본인 카드에 맞추고, 혜택·구간(`benefits`·`tiers`)도 본인 카드 것이 온다.

import type { RefMaps } from '../api/hooks'
import type { PaymentMethod } from '../api/types'

type CardLike = Pick<PaymentMethod, 'id' | 'parentId'>

/** 이 카드의 가족카드들 (기준 데이터 순서) */
export function familyCardsOf(refMaps: RefMaps, id: number): PaymentMethod[] {
  return refMaps.paymentMethods.filter((p) => p.parentId === id)
}

/** 본인 카드 바로 뒤에 그 가족카드가 오도록 순서를 바꾼다. 본인 카드가 목록에 없는 가족카드는 제자리. */
export function withFamilyOrder<T>(items: T[], cardOf: (item: T) => CardLike | undefined): T[] {
  const ids = new Set(items.map((i) => cardOf(i)?.id))
  const out: T[] = []
  for (const item of items) {
    const parentId = cardOf(item)?.parentId
    if (parentId != null && ids.has(parentId)) continue
    const id = cardOf(item)?.id
    out.push(item, ...items.filter((f) => id != null && cardOf(f)?.parentId === id))
  }
  return out
}

/** 실적·혜택 리포트에 붙이는 설명: 가족카드는 본인 카드 실적을 같이 쓰고, 본인 카드 실적엔 가족카드 이용이 들어간다 */
export function familyReportNote(card: PaymentMethod, refMaps: RefMaps): string | null {
  if (card.parentId != null) {
    const parent = refMaps.paymentMethodById.get(card.parentId)
    return `가족카드 · 본인 카드 ${parent?.name ?? ''}의 실적·혜택을 함께 씁니다 (본인 카드 이용 포함)`
  }
  const family = familyCardsOf(refMaps, card.id)
  return family.length ? `가족카드 ${family.map((f) => f.name).join(', ')} 이용 포함` : null
}

/** 청구서 설명: 가족카드 대금은 본인 카드 청구서에 합쳐 청구된다 (서버 `statement_card_ids`) */
export function familyStatementNote(card: PaymentMethod, refMaps: RefMaps): string | null {
  if (card.parentId != null) {
    const parent = refMaps.paymentMethodById.get(card.parentId)
    return `이 카드 이용분 · 본인 카드 ${parent?.name ?? ''} 청구서에 합산되어 출금`
  }
  const family = familyCardsOf(refMaps, card.id)
  return family.length ? `가족카드 ${family.map((f) => f.name).join(', ')} 이용 포함` : null
}
