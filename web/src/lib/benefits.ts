// 카드 혜택 표시·선택 규칙

import type { BenefitAmount, BenefitKind, CardBenefit, CardMonthReport } from '../api/types'
import { ratePercent, won } from './format'

export const BENEFIT_KINDS: BenefitKind[] = ['DISCOUNT', 'POINT', 'MILEAGE']

export const BENEFIT_KIND_LABEL: Record<BenefitKind, string> = {
  DISCOUNT: '할인',
  POINT: '포인트 적립',
  MILEAGE: '마일리지 적립',
}

/** 혜택액 단위 (포인트는 1P = 1원으로 본다) */
export const BENEFIT_UNIT: Record<BenefitKind, string> = { DISCOUNT: '원', POINT: 'P', MILEAGE: '마일' }

const decimal = new Intl.NumberFormat('ko-KR', { maximumFractionDigits: 2 })

/** 마일리지 혜택률은 "N원당 1마일"로 입력한다: 1/1500 → "1500" */
export function rateToWonPerMile(rate: number): string {
  return rate > 0 ? String(Number((1 / rate).toFixed(2))) : ''
}

/** "1500" → 1/1500 */
export function wonPerMileToRate(input: string): number {
  const n = Number(input)
  return n > 0 ? 1 / n : 0
}

/** "1% 할인" / "0.8% 적립" / "1,500원당 1마일" */
export function benefitRateText(b: { kind: BenefitKind; rate: number }): string {
  if (b.kind === 'MILEAGE') return b.rate > 0 ? `${decimal.format(1 / b.rate)}원당 1마일` : '-'
  return `${ratePercent(b.rate)} ${b.kind === 'POINT' ? '적립' : '할인'}`
}

/** 1,200원 / 1,200P / 35마일 */
export function benefitAmountText(kind: BenefitKind, amount: number): string {
  return `${won(amount)}${BENEFIT_UNIT[kind]}`
}

/** 표 칸용: 할인은 숫자만, 포인트·마일리지는 단위를 붙인다 */
export function benefitValueText(kind: BenefitKind, amount: number): string {
  return kind === 'DISCOUNT' ? won(amount) : benefitAmountText(kind, amount)
}

type Totals = Pick<CardMonthReport, 'discountTotal' | 'pointTotal' | 'mileageTotal'>

/** 예정 거래가 이번 달 실적에 더하는 금액 (예정 포함 전망 − 확정 실적). 예정 환불이면 음수. */
export function plannedPerformance(r: Pick<CardMonthReport, 'performance' | 'projectedPerformance'>): number {
  return r.projectedPerformance - r.performance
}

export function hasBenefit(r: Totals): boolean {
  return r.discountTotal !== 0 || r.pointTotal !== 0 || r.mileageTotal !== 0
}

/** 종류별 합계 "3,200원 · 1,200P · 35마일" (0인 종류는 빼고, 모두 0이면 "0원") */
export function benefitTotalsText(r: Totals): string {
  const parts: [BenefitKind, number][] = [
    ['DISCOUNT', r.discountTotal],
    ['POINT', r.pointTotal],
    ['MILEAGE', r.mileageTotal],
  ]
  const text = parts.filter(([, v]) => v !== 0).map(([k, v]) => benefitAmountText(k, v))
  return text.length ? text.join(' · ') : '0원'
}

/** 예정 거래까지 받은 혜택의 종류별 합계 (`benefitTotalsText`에 넘긴다) */
export function projectedTotals(r: CardMonthReport): Totals {
  return { discountTotal: r.projectedDiscountTotal, pointTotal: r.projectedPointTotal, mileageTotal: r.projectedMileageTotal }
}

/** 예정 거래가 혜택을 바꾸는지 */
export function benefitsChangedByPlanned(r: CardMonthReport): boolean {
  return r.projectedDiscountTotal !== r.discountTotal || r.projectedPointTotal !== r.pointTotal || r.projectedMileageTotal !== r.mileageTotal
}

/** 남은 혜택 "6,800원 · 990마일" (한도 있는 혜택이 없으면 null) */
export function remainingBenefitsText(list: BenefitAmount[]): string | null {
  return list.length ? list.map((b) => benefitAmountText(b.kind, b.amount)).join(' · ') : null
}

/** 카드 목록의 남은 혜택: "6,800원 (예정 포함 4,000원)". 혜택이 없는 카드는 null, 한도 있는 혜택이 없으면 "제한 없음" */
export function remainingSummary(r: CardMonthReport): string | null {
  if (r.benefits.length === 0) return null
  const now = remainingBenefitsText(r.remainingBenefits)
  if (now == null) return '제한 없음'
  const projected = remainingBenefitsText(r.projectedRemainingBenefits)
  return projected != null && projected !== now ? `${now} (예정 포함 ${projected})` : now
}

/** 거래별 혜택액 (그리드 "혜택액" 칸) */
export function txBenefitTexts(r: CardMonthReport): Map<number, string> {
  const parts = new Map<number, string[]>()
  for (const t of r.transactionBenefits) {
    if (t.amount === 0) continue
    const list = parts.get(t.transactionId) ?? []
    list.push(benefitValueText(t.kind, t.amount))
    parts.set(t.transactionId, list)
  }
  return new Map([...parts].map(([id, list]) => [id, list.join(' · ')]))
}

/** 통합 한도에서 차감되지 않는 혜택의 한도 방식 (차감되면 null) */
export function separateLimitLabel(b: { sharesCardLimit: boolean; monthlyCap: number | null }): string | null {
  if (b.sharesCardLimit) return null
  return b.monthlyCap != null ? '별도 한도' : '한도 없음'
}

/** 혜택 선택을 바꾼다. 중복 적용이 아닌 혜택을 고르면 다른 비중복 혜택은 해제한다 (거래당 하나). */
export function toggleBenefit(selected: number[], benefit: CardBenefit, checked: boolean, all: CardBenefit[]): number[] {
  const rest = selected.filter((id) => id !== benefit.id)
  if (!checked) return rest
  if (benefit.stackable) return [...rest, benefit.id]
  const exclusive = new Set(all.filter((b) => !b.stackable).map((b) => b.id))
  return [...rest.filter((id) => !exclusive.has(id)), benefit.id]
}
