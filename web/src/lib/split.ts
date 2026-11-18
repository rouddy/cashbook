import type { Transaction } from '../api/types'
import { won } from './format'

/** 여러 장부에 나눈 결제의 한쪽인지 */
export function isSplit(tx: Transaction): boolean {
  return tx.splitWith.length > 0
}

/** 나눈 쪽을 모두 합친 실제 결제 금액 */
export function splitTotal(tx: Transaction): number {
  return tx.splitWith.reduce((sum, p) => sum + p.amount, tx.amount)
}

/** 분할을 되돌리면 합쳐지는 원래 거래의 장부 */
export function splitOriginLedgerId(tx: Transaction): number {
  if (tx.splitParentId == null) return tx.ledgerId
  return tx.splitWith.find((p) => p.id === tx.splitParentId)?.ledgerId ?? tx.ledgerId
}

/** 결제 금액 `total`에서 `part`를 떼어낼 수 있는지 (서버 `split::check_part`와 같은 규칙). 안 되면 이유 */
export function splitAmountError(total: number, part: number | null): string | null {
  if (total === 0) return '0원 거래는 나눌 수 없습니다'
  if (part == null) return '금액을 숫자로 입력하세요'
  if (part === 0 || Math.sign(part) !== Math.sign(total) || Math.abs(part) >= Math.abs(total)) {
    return total > 0
      ? `1원 이상, 결제 금액 ${won(total)}원보다 작게 입력하세요`
      : `환불은 음수로, 절댓값이 ${won(-total)}원보다 작게 입력하세요`
  }
  return null
}
