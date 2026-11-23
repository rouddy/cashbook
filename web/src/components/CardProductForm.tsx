import { useState, type FormEvent } from 'react'

import { useMutate } from '../api/hooks'
import { CREATE_CARD_PRODUCT, UPDATE_CARD_PRODUCT } from '../api/queries'
import type { BenefitKind, CardProduct } from '../api/types'
import { BENEFIT_KIND_LABEL, BENEFIT_KINDS, BENEFIT_UNIT, rateToWonPerMile, wonPerMileToRate } from '../lib/benefits'
import { ISSUERS } from '../lib/cards'
import { parseAmount, percentToRate, rateToPercent, won } from '../lib/format'
import { useToast } from './Toast'

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

/**
 * 카드 상품(전월실적 목표·구간·혜택·연회비) 추가·수정. 상품은 모든 계정이 함께 쓰므로
 * 고치면 그 상품을 고른 모든 카드의 실적·혜택 계산이 바뀐다 (만든 계정과 관리자만 고칠 수 있다).
 */
export default function CardProductForm({
  product,
  initialIssuer,
  onDone,
  onCancel,
}: {
  product?: CardProduct
  /** 새 상품의 카드사 (카드 폼에서 열 때) */
  initialIssuer?: string
  onDone: (id: number) => void
  onCancel?: () => void
}) {
  const toast = useToast()
  const { run, pending } = useMutate()
  const [issuer, setIssuer] = useState(product?.issuer ?? initialIssuer ?? '')
  const [name, setName] = useState(product?.name ?? '')
  const [annualFee, setAnnualFee] = useState(numStr(product?.annualFee))
  const [monthlyTarget, setMonthlyTarget] = useState(numStr(product?.monthlyTarget))
  const [memo, setMemo] = useState(product?.memo ?? '')
  const [benefits, setBenefits] = useState<BenefitRow[]>(
    (product?.benefits ?? []).map((b) => ({
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
  const [tiers, setTiers] = useState<TierRow[]>((product?.tiers ?? []).map((t) => ({ minSpending: String(t.minSpending), benefitLimit: String(t.benefitLimit) })))

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    const amountOrNull = (s: string) => (s.trim() === '' ? null : parseAmount(s))
    if (!name.trim()) {
      toast('상품명을 입력하세요', 'error')
      return
    }
    // 이름만 지운 기존 혜택이 조용히 삭제되지 않게 막는다 (지우려면 행의 삭제 버튼)
    if (benefits.some((b) => !b.name.trim() && (b.id != null || b.rateInput.trim() !== ''))) {
      toast('혜택 이름을 입력하세요. 혜택을 없애려면 그 행의 삭제를 누르세요', 'error')
      return
    }
    const kept = new Set(benefits.map((b) => b.id).filter((id) => id != null))
    const removed = (product?.benefits ?? []).filter((b) => !kept.has(b.id))
    if (product && removed.length > 0 && product.cardCount > 0) {
      const msg = `혜택 ${removed.map((b) => `'${b.name}'`).join(', ')}을(를) 지우면 이 상품을 쓰는 모든 계정의 거래·반복 규칙에서도 그 혜택 표시가 빠집니다. 계속할까요?`
      if (!window.confirm(msg)) return
    }
    const input = {
      issuer: issuer.trim() || null,
      name: name.trim(),
      annualFee: amountOrNull(annualFee),
      monthlyTarget: amountOrNull(monthlyTarget),
      memo: memo.trim() || null,
      benefits: benefits
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
        })),
      tiers: tiers
        .filter((t) => t.minSpending.trim() && t.benefitLimit.trim())
        .map((t) => ({ minSpending: parseAmount(t.minSpending) ?? 0, benefitLimit: parseAmount(t.benefitLimit) ?? 0 })),
    }
    if (product) {
      if (!(await run(UPDATE_CARD_PRODUCT, { id: product.id, input }))) return
      toast('카드 상품을 저장했습니다')
      onDone(product.id)
    } else {
      const r = await run<{ createCardProduct: { id: number } }>(CREATE_CARD_PRODUCT, { input })
      if (!r) return
      toast('카드 상품을 추가했습니다')
      onDone(r.createCardProduct.id)
    }
  }

  return (
    <form className="stack" onSubmit={submit}>
      <div className="notice info">
        <span className="notice-icon">i</span>
        <span>
          카드 상품은 모든 계정이 함께 씁니다.
          {product && product.cardCount > 0 ? ` 지금 카드 ${product.cardCount}장(다른 계정 포함)이 이 상품을 쓰고 있어, 고치면 바로 반영됩니다.` : ' 카드 명세서·상품 안내를 보고 입력하세요.'}
        </span>
      </div>
      <div className="form-grid">
        <label className="field">
          카드사
          <input className="input" list="product-issuer-list" value={issuer} onChange={(e) => setIssuer(e.target.value)} />
          <datalist id="product-issuer-list">
            {ISSUERS.map((i) => (
              <option key={i} value={i} />
            ))}
          </datalist>
        </label>
        <label className="field">
          상품명 *
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="예: MG+ S" required />
        </label>
        <label className="field">
          연회비
          <input className="input num" inputMode="numeric" value={annualFee} onChange={(e) => setAnnualFee(e.target.value)} />
        </label>
        <label className="field">
          전월실적 목표
          <input className="input num" inputMode="numeric" value={monthlyTarget} onChange={(e) => setMonthlyTarget(e.target.value)} placeholder="예: 300000" />
        </label>
      </div>
      <label className="field">
        메모
        <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} placeholder="예: 2026년 상품설명서 기준" />
      </label>

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

      <div className="row">
        <button className="btn primary" disabled={pending}>
          {product ? '상품 저장' : '상품 추가'}
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
