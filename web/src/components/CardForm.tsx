import { useState, type FormEvent } from 'react'

import { gql } from '../api/client'
import { type RefMaps, useMutate } from '../api/hooks'
import { CREATE_PM, SUGGEST_BILLING, UPDATE_PM } from '../api/queries'
import type { BenefitKind, PaymentKind, PaymentMethod } from '../api/types'
import { BENEFIT_KIND_LABEL, BENEFIT_KINDS, BENEFIT_UNIT, rateToWonPerMile, wonPerMileToRate } from '../lib/benefits'
import { parseAmount, percentToRate, rateToPercent, won } from '../lib/format'
import { useToast } from './Toast'

const ISSUERS = ['KB국민', '신한', '현대', '삼성', '롯데', '하나', '우리', 'BC', 'NH농협', 'IBK기업', '씨티', '카카오뱅크', '토스']

interface BenefitRow {
  id?: number
  name: string
  kind: BenefitKind
  /** 할인·포인트는 %, 마일리지는 "N원당 1마일"의 N */
  rateInput: string
  minAmount: string
  perTxMax: string
  monthlyCap: string
  sharesCardLimit: boolean
  excludesPerformance: boolean
  stackable: boolean
}

const rateToInput = (kind: BenefitKind, rate: number) => (kind === 'MILEAGE' ? rateToWonPerMile(rate) : rateToPercent(rate))
const inputToRate = (kind: BenefitKind, input: string) => (kind === 'MILEAGE' ? wonPerMileToRate(input) : percentToRate(input))

interface TierRow {
  minSpending: string
  benefitLimit: string
}

const numStr = (v: number | null | undefined) => (v == null ? '' : String(v))

/** 이용기간 설명 (서버 usage_period_label과 같은 규칙) */
function usagePeriodText(closingDay: number, offset: number): string {
  const label = (o: number) => (o === 0 ? '당월' : o === -1 ? '전월' : o === -2 ? '전전월' : '전전전월')
  if (closingDay >= 31) return `${label(offset)} 1일 ~ ${label(offset)} 말일`
  return `${label(offset - 1)} ${closingDay + 1}일 ~ ${label(offset)} ${closingDay}일`
}

export default function CardForm({
  card,
  refMaps,
  kinds = ['CREDIT_CARD', 'CHECK_CARD'],
  onDone,
  onCancel,
}: {
  card?: PaymentMethod
  refMaps: RefMaps
  kinds?: PaymentKind[]
  onDone: (id: number) => void
  onCancel?: () => void
}) {
  const toast = useToast()
  const { run, pending } = useMutate()
  const [name, setName] = useState(card?.name ?? '')
  const [kind, setKind] = useState<PaymentKind>(card?.kind ?? kinds[0])
  const [issuer, setIssuer] = useState(card?.issuer ?? '')
  const [productName, setProductName] = useState(card?.productName ?? '')
  const [last4, setLast4] = useState(card?.last4 ?? '')
  const [aliases, setAliases] = useState((card?.aliases ?? []).join(', '))
  const [accountId, setAccountId] = useState(numStr(card?.accountId))
  const [paymentDay, setPaymentDay] = useState(numStr(card?.paymentDay))
  const [closingDay, setClosingDay] = useState(numStr(card?.closingDay))
  const [closingOffset, setClosingOffset] = useState(card?.closingMonthOffset != null ? String(card.closingMonthOffset) : '-1')
  const [monthlyTarget, setMonthlyTarget] = useState(numStr(card?.monthlyTarget))
  const [annualFee, setAnnualFee] = useState(numStr(card?.annualFee))
  const [expiresOn, setExpiresOn] = useState(card?.expiresOn ?? '')
  const [memo, setMemo] = useState(card?.memo ?? '')
  const [benefits, setBenefits] = useState<BenefitRow[]>(
    (card?.benefits ?? []).map((b) => ({
      id: b.id,
      name: b.name,
      kind: b.kind,
      rateInput: rateToInput(b.kind, b.rate),
      minAmount: numStr(b.minAmount),
      perTxMax: numStr(b.perTxMax),
      monthlyCap: numStr(b.monthlyCap),
      sharesCardLimit: b.sharesCardLimit,
      excludesPerformance: b.excludesPerformance,
      stackable: b.stackable,
    })),
  )
  const [tiers, setTiers] = useState<TierRow[]>((card?.tiers ?? []).map((t) => ({ minSpending: String(t.minSpending), benefitLimit: String(t.benefitLimit) })))

  const isCredit = kind === 'CREDIT_CARD'
  const isCard = kind === 'CREDIT_CARD' || kind === 'CHECK_CARD'
  const billingFilled = paymentDay !== '' && closingDay !== ''

  const suggest = async () => {
    if (!paymentDay) {
      toast('결제일을 먼저 고르세요', 'error')
      return
    }
    const r = await gql<{ suggestedBilling: { closingDay: number; closingMonthOffset: number } }>(SUGGEST_BILLING, { day: Number(paymentDay) })
    setClosingDay(String(r.suggestedBilling.closingDay))
    setClosingOffset(String(r.suggestedBilling.closingMonthOffset))
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    const amountOrNull = (s: string) => (s.trim() === '' ? null : parseAmount(s))
    const input = {
      name: name.trim(),
      kind,
      issuer: issuer || null,
      productName: productName || null,
      last4: last4 || null,
      aliases: aliases.split(',').map((a) => a.trim()).filter(Boolean),
      accountId: accountId ? Number(accountId) : null,
      paymentDay: isCredit && billingFilled ? Number(paymentDay) : null,
      closingDay: isCredit && billingFilled ? Number(closingDay) : null,
      closingMonthOffset: isCredit && billingFilled ? Number(closingOffset) : null,
      monthlyTarget: amountOrNull(monthlyTarget),
      annualFee: amountOrNull(annualFee),
      expiresOn: expiresOn || null,
      memo: memo || null,
      benefits: isCard
        ? benefits
            .filter((b) => b.name.trim())
            .map((b) => ({
              id: b.id ?? null,
              name: b.name.trim(),
              kind: b.kind,
              rate: inputToRate(b.kind, b.rateInput),
              minAmount: amountOrNull(b.minAmount),
              perTxMax: amountOrNull(b.perTxMax),
              monthlyCap: amountOrNull(b.monthlyCap),
              sharesCardLimit: b.sharesCardLimit,
              excludesPerformance: b.excludesPerformance,
              stackable: b.stackable,
            }))
        : [],
      tiers: isCard
        ? tiers
            .filter((t) => t.minSpending.trim() && t.benefitLimit.trim())
            .map((t) => ({ minSpending: parseAmount(t.minSpending) ?? 0, benefitLimit: parseAmount(t.benefitLimit) ?? 0 }))
        : [],
    }
    if (!input.name) {
      toast('이름을 입력하세요', 'error')
      return
    }
    // 이름만 지운 기존 혜택이 조용히 삭제되지 않게 막는다 (지우려면 행의 삭제 버튼)
    if (isCard && benefits.some((b) => !b.name.trim() && (b.id != null || b.rateInput.trim() !== ''))) {
      toast('혜택 이름을 입력하세요. 혜택을 없애려면 그 행의 삭제를 누르세요', 'error')
      return
    }
    if (isCredit && (paymentDay === '') !== (closingDay === '')) {
      toast('정산 일정은 결제일과 마감일을 모두 입력하세요', 'error')
      return
    }
    if (card) {
      if (!(await run(UPDATE_PM, { id: card.id, input }))) return
      toast('저장했습니다')
      onDone(card.id)
    } else {
      const r = await run<{ createPaymentMethod: { id: number } }>(CREATE_PM, { input })
      if (!r) return
      toast('추가했습니다')
      onDone(r.createPaymentMethod.id)
    }
  }

  return (
    <form className="stack" onSubmit={submit}>
      <div className="form-grid">
        <label className="field">
          이름 *
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="예: MG+S" required />
        </label>
        <label className="field">
          종류
          <select className="input" value={kind} onChange={(e) => setKind(e.target.value as PaymentKind)}>
            {kinds.map((k) => (
              <option key={k} value={k}>
                {{ CREDIT_CARD: '신용카드', CHECK_CARD: '체크카드', CASH: '현금', BANK_TRANSFER: '계좌이체', OTHER: '기타' }[k]}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          {isCard ? '카드사' : '은행/기관'}
          <input className="input" list="issuer-list" value={issuer} onChange={(e) => setIssuer(e.target.value)} />
          <datalist id="issuer-list">
            {ISSUERS.map((i) => (
              <option key={i} value={i} />
            ))}
          </datalist>
        </label>
        {isCard && (
          <label className="field">
            상품명
            <input className="input" value={productName} onChange={(e) => setProductName(e.target.value)} placeholder="예: MG+ S 하나카드" />
          </label>
        )}
        {isCard && (
          <label className="field">
            카드번호 끝 4자리
            <input className="input" inputMode="numeric" maxLength={4} value={last4} onChange={(e) => setLast4(e.target.value.replace(/\D/g, ''))} />
            <span className="hint">승인 문자 자동 입력 시 카드 구분에 씀</span>
          </label>
        )}
        <label className="field">
          {isCard ? '결제 계좌' : '연결 계좌'}
          <select className="input" value={accountId} onChange={(e) => setAccountId(e.target.value)}>
            <option value="">선택 안 함</option>
            {refMaps.accounts
              .filter((a) => !a.archived || String(a.id) === accountId)
              .map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
          </select>
          <span className="hint">여유자금 계산 시 이 계좌의 잔고 기준일을 씀</span>
        </label>
      </div>

      {isCredit && (
        <fieldset className="panel" style={{ padding: 14, border: '1px solid var(--border)' }}>
          <legend style={{ fontWeight: 650, padding: '0 6px' }}>정산 일정</legend>
          <div className="form-grid" style={{ alignItems: 'end' }}>
            <label className="field">
              결제일
              <select className="input" value={paymentDay} onChange={(e) => setPaymentDay(e.target.value)}>
                <option value="">미설정</option>
                {Array.from({ length: 31 }, (_, i) => i + 1).map((d) => (
                  <option key={d} value={d}>
                    매월 {d}일
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              이용기간 마감월
              <select className="input" value={closingOffset} onChange={(e) => setClosingOffset(e.target.value)}>
                <option value="0">결제월 당월</option>
                <option value="-1">결제월 전월</option>
                <option value="-2">결제월 전전월</option>
              </select>
            </label>
            <label className="field">
              이용기간 마감일
              <select className="input" value={closingDay} onChange={(e) => setClosingDay(e.target.value)}>
                <option value="">미설정</option>
                {Array.from({ length: 30 }, (_, i) => i + 1).map((d) => (
                  <option key={d} value={d}>
                    {d}일
                  </option>
                ))}
                <option value="31">말일</option>
              </select>
            </label>
            <button type="button" className="btn" onClick={suggest}>
              결제일로 자동 추정
            </button>
          </div>
          <p className="small text-2" style={{ margin: '10px 0 0' }}>
            {billingFilled
              ? `매월 ${paymentDay}일 결제 · 이용기간 ${usagePeriodText(Number(closingDay), Number(closingOffset))} (주말이면 다음 영업일 출금)`
              : '카드 명세서의 "이용기간"을 보고 입력하세요. 결제일만 고르고 자동 추정을 누르면 카드사 일반 기준(결제일 약 14일 전 마감)으로 채웁니다.'}
          </p>
        </fieldset>
      )}

      <div className="form-grid">
        {isCard && (
          <label className="field">
            전월실적 목표
            <input className="input num" inputMode="numeric" value={monthlyTarget} onChange={(e) => setMonthlyTarget(e.target.value)} placeholder="예: 300000" />
          </label>
        )}
        {isCard && (
          <label className="field">
            연회비
            <input className="input num" inputMode="numeric" value={annualFee} onChange={(e) => setAnnualFee(e.target.value)} />
          </label>
        )}
        {isCard && (
          <label className="field">
            유효기간
            <input className="input" type="month" value={expiresOn} onChange={(e) => setExpiresOn(e.target.value)} />
          </label>
        )}
        <label className="field">
          자동 입력 별칭 (쉼표로 구분)
          <input className="input" value={aliases} onChange={(e) => setAliases(e.target.value)} placeholder="예: 알뜰교통 신한카드" />
        </label>
      </div>
      <label className="field">
        메모
        <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} />
      </label>

      {isCard && (
        <>
          <div>
            <div className="row" style={{ marginBottom: 6 }}>
              <h3>혜택</h3>
              <span className="small muted">카드 명세의 할인/적립 항목. 거래에 혜택을 표시하면 혜택액과 남은 한도를 계산합니다.</span>
            </div>
            <div className="table-wrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>이름</th>
                    <th>종류</th>
                    <th className="num">혜택률</th>
                    <th className="num">건당 최소결제</th>
                    <th className="num">건당 최대</th>
                    <th className="num">항목 월한도</th>
                    <th>통합한도 차감</th>
                    <th>중복 적용</th>
                    <th>실적 제외</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {benefits.map((b, i) => {
                    const upd = (patch: Partial<BenefitRow>) => setBenefits((list) => list.map((x, j) => (j === i ? { ...x, ...patch } : x)))
                    // 종류를 바꾸면 같은 혜택률을 새 단위로 다시 적는다 (% ↔ N원당 1마일)
                    const changeKind = (kind: BenefitKind) =>
                      upd({ kind, rateInput: b.rateInput.trim() === '' ? '' : rateToInput(kind, inputToRate(b.kind, b.rateInput)) })
                    return (
                      <tr key={b.id ?? `new-${i}`}>
                        <td>
                          <input className="input" value={b.name} onChange={(e) => upd({ name: e.target.value })} style={{ width: 130 }} />
                        </td>
                        <td>
                          <select className="input" value={b.kind} onChange={(e) => changeKind(e.target.value as BenefitKind)} aria-label="혜택 종류">
                            {BENEFIT_KINDS.map((k) => (
                              <option key={k} value={k}>
                                {BENEFIT_KIND_LABEL[k]}
                              </option>
                            ))}
                          </select>
                        </td>
                        <td className="nowrap">
                          <input
                            className="input num"
                            inputMode="decimal"
                            value={b.rateInput}
                            onChange={(e) => upd({ rateInput: e.target.value })}
                            style={{ width: 70 }}
                            aria-label={b.kind === 'MILEAGE' ? '1마일당 이용금액' : '혜택률(%)'}
                          />{' '}
                          <span className="small text-2">{b.kind === 'MILEAGE' ? '원당 1마일' : '%'}</span>
                        </td>
                        <td>
                          <input className="input num" inputMode="numeric" value={b.minAmount} onChange={(e) => upd({ minAmount: e.target.value })} style={{ width: 90 }} />
                        </td>
                        <td>
                          <input
                            className="input num"
                            inputMode="numeric"
                            value={b.perTxMax}
                            onChange={(e) => upd({ perTxMax: e.target.value })}
                            style={{ width: 80 }}
                            placeholder={BENEFIT_UNIT[b.kind]}
                          />
                        </td>
                        <td>
                          <input
                            className="input num"
                            inputMode="numeric"
                            value={b.monthlyCap}
                            onChange={(e) => upd({ monthlyCap: e.target.value })}
                            style={{ width: 90 }}
                            placeholder={BENEFIT_UNIT[b.kind]}
                          />
                        </td>
                        <td>
                          <input type="checkbox" checked={b.sharesCardLimit} onChange={(e) => upd({ sharesCardLimit: e.target.checked })} aria-label="통합한도 차감" />
                        </td>
                        <td>
                          <input type="checkbox" checked={b.stackable} onChange={(e) => upd({ stackable: e.target.checked })} aria-label="다른 혜택과 중복 적용" />
                        </td>
                        <td>
                          <input type="checkbox" checked={b.excludesPerformance} onChange={(e) => upd({ excludesPerformance: e.target.checked })} aria-label="혜택받은 금액 실적 제외" />
                        </td>
                        <td>
                          <button type="button" className="btn sm ghost danger" onClick={() => setBenefits((list) => list.filter((_, j) => j !== i))}>
                            삭제
                          </button>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
            <button
              type="button"
              className="btn sm"
              style={{ marginTop: 6 }}
              onClick={() =>
                setBenefits((list) => [
                  ...list,
                  {
                    name: '',
                    kind: 'DISCOUNT',
                    rateInput: '',
                    minAmount: '',
                    perTxMax: '',
                    monthlyCap: '',
                    sharesCardLimit: true,
                    excludesPerformance: false,
                    stackable: false,
                  },
                ])
              }
            >
              + 혜택 추가
            </button>
            <ul className="small text-2" style={{ margin: '8px 0 0', paddingLeft: 18 }}>
              <li>
                <b>통합한도 차감</b>: 전월실적 구간 한도에서 빠지는 혜택. 끄고 항목 월한도도 비우면 한도 없이 받습니다(예: 기본 0.8% 적립).
              </li>
              <li>
                <b>중복 적용</b>: 다른 혜택과 함께 받는 혜택(예: 백화점 1% + 간편결제 3%). 중복 적용이 아닌 혜택은 거래당 하나만 고릅니다.
              </li>
              <li>건당 최대·항목 월한도는 혜택 단위(할인 원, 포인트 P, 마일리지 마일)로 적습니다.</li>
            </ul>
          </div>
          <div>
            <div className="row" style={{ marginBottom: 6 }}>
              <h3>전월실적 구간</h3>
              <span className="small muted">지난달 실적이 이 금액 이상이면 이번 달 통합 혜택 한도</span>
            </div>
            {tiers.map((t, i) => {
              const upd = (patch: Partial<TierRow>) => setTiers((list) => list.map((x, j) => (j === i ? { ...x, ...patch } : x)))
              return (
                <div key={i} className="row" style={{ marginBottom: 6 }}>
                  <input className="input num" inputMode="numeric" value={t.minSpending} onChange={(e) => upd({ minSpending: e.target.value })} style={{ width: 130 }} aria-label="전월실적 이상" />
                  <span className="text-2">원 이상 →</span>
                  <input className="input num" inputMode="numeric" value={t.benefitLimit} onChange={(e) => upd({ benefitLimit: e.target.value })} style={{ width: 110 }} aria-label="혜택 한도" />
                  <span className="text-2">원 한도</span>
                  <button type="button" className="btn sm ghost danger" onClick={() => setTiers((list) => list.filter((_, j) => j !== i))}>
                    삭제
                  </button>
                  {parseAmount(t.minSpending) != null && <span className="small muted">({won(parseAmount(t.minSpending))}원)</span>}
                </div>
              )
            })}
            <button type="button" className="btn sm" onClick={() => setTiers((list) => [...list, { minSpending: '', benefitLimit: '' }])}>
              + 구간 추가
            </button>
          </div>
        </>
      )}

      <div className="row">
        <button className="btn primary" disabled={pending}>
          {card ? '저장' : '추가'}
        </button>
        {onCancel && (
          <button type="button" className="btn ghost" onClick={onCancel}>
            취소
          </button>
        )}
      </div>
    </form>
  )
}
