// 금액/날짜 표시

const nf = new Intl.NumberFormat('ko-KR')

/** 1234567 → "1,234,567" */
export function won(n: number | null | undefined): string {
  if (n == null || Number.isNaN(n)) return '-'
  return nf.format(Math.round(n))
}

/** 부호 포함: +12,000 / -3,000 */
export function signedWon(n: number): string {
  const s = won(Math.abs(n))
  if (n > 0) return `+${s}`
  if (n < 0) return `-${s}`
  return s
}

/** 축 눈금용: 0 / 50만 / 1,200만 / 1.2억 */
export function compactWon(n: number): string {
  const abs = Math.abs(n)
  const sign = n < 0 ? '-' : ''
  if (abs >= 100_000_000) {
    const v = abs / 100_000_000
    return `${sign}${v >= 10 ? Math.round(v) : Math.round(v * 10) / 10}억`
  }
  if (abs >= 10_000) {
    return `${sign}${nf.format(Math.round(abs / 10_000))}만`
  }
  return `${sign}${nf.format(Math.round(abs))}`
}

/** 혜택률 0.008 → "0.8%", 0.015 → "1.5%" */
export function ratePercent(rate: number): string {
  return `${Number((rate * 100).toFixed(3))}%`
}

/** "2.8"(%) → 0.028. 부동소수 오차 없이 */
export function percentToRate(input: string): number {
  return Math.round(Number(input || 0) * 1000) / 100000
}

/** 0.028 → "2.8" */
export function rateToPercent(rate: number): string {
  return String(Number((rate * 100).toFixed(3)))
}

export function percent(n: number, digits = 0): string {
  if (!Number.isFinite(n)) return '-'
  return `${(n * 100).toFixed(digits)}%`
}

const WEEKDAYS = ['일', '월', '화', '수', '목', '금', '토']

export function parseDate(s: string): Date {
  const [y, m, d] = s.split('-').map(Number)
  return new Date(y, m - 1, d)
}

/** "2026-10-23" → "10/23 (금)" */
export function shortDate(s: string | null | undefined, withWeekday = true): string {
  if (!s) return '-'
  const d = parseDate(s)
  const base = `${d.getMonth() + 1}/${d.getDate()}`
  return withWeekday ? `${base} (${WEEKDAYS[d.getDay()]})` : base
}

/** "2026-10-23" → "2026. 10. 23." */
export function longDate(s: string | null | undefined): string {
  if (!s) return '-'
  const d = parseDate(s)
  return `${d.getFullYear()}. ${d.getMonth() + 1}. ${d.getDate()}.`
}

export function weekday(s: string): string {
  return WEEKDAYS[parseDate(s).getDay()]
}

export function isWeekend(s: string): boolean {
  const day = parseDate(s).getDay()
  return day === 0 || day === 6
}

/** 달력처럼 토요일은 파랑, 일요일은 빨강 */
export function weekendClass(s: string): string {
  const day = parseDate(s).getDay()
  return day === 0 ? 'bad' : day === 6 ? 'sat' : ''
}

/** 금액 입력값 해석: "12,500" → 12500, "-3000" → -3000. 숫자가 없으면 null */
export function parseAmount(input: string): number | null {
  const cleaned = input.replace(/[,\s원]/g, '')
  if (!cleaned || !/^-?\d+$/.test(cleaned)) return null
  return Number(cleaned)
}

export function dateTime(s: string | null | undefined): string {
  if (!s) return '-'
  const d = new Date(s)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')} ${String(
    d.getHours(),
  ).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

export const PAYMENT_KIND_LABEL: Record<string, string> = {
  CREDIT_CARD: '신용카드',
  CHECK_CARD: '체크카드',
  CASH: '현금',
  BANK_TRANSFER: '계좌이체',
  OTHER: '기타',
}

export const ACCOUNT_KIND_LABEL: Record<string, string> = {
  CHECKING: '입출금',
  SAVINGS: '적금/청약',
  DEPOSIT: '예금',
  INVESTMENT: '투자',
  CASH: '현금',
  OTHER: '기타',
}

export const STATUS_LABEL: Record<string, string> = {
  CONFIRMED: '확정',
  PLANNED: '예정',
  SKIPPED: '건너뜀',
}

export const SOURCE_LABEL: Record<string, string> = {
  MANUAL: '직접 입력',
  IMPORT: '스프레드시트',
  API: '자동 입력',
  RECURRING: '반복 규칙',
}

export const PLAN_KIND_LABEL: Record<string, string> = {
  ALLOCATION: '배분',
  MEMBER_ALLOWANCE: '구성원 용돈',
}

export const OFFSET_LABEL: Record<number, string> = { 0: '당월', [-1]: '전월', [-2]: '전전월' }
