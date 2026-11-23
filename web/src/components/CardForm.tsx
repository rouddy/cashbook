import { useState, type FormEvent } from 'react'

import { gql } from '../api/client'
import { type RefMaps, useGql, useMutate } from '../api/hooks'
import { CARD_PRODUCTS_QUERY, CREATE_PM, SUGGEST_BILLING, UPDATE_PM } from '../api/queries'
import type { CardProduct, PaymentKind, PaymentMethod } from '../api/types'
import { familyCardsOf, ISSUERS } from '../lib/cards'
import { PAYMENT_KIND_LABEL, won } from '../lib/format'
import CardProductForm from './CardProductForm'
import { useToast } from './Toast'

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
  familyOf,
  onDone,
  onCancel,
}: {
  card?: PaymentMethod
  refMaps: RefMaps
  kinds?: PaymentKind[]
  /** 새 카드를 이 카드의 가족카드로 추가한다 (가족카드 체크·본인 카드 선택이 된 채로 연다) */
  familyOf?: number
  onDone: (id: number) => void
  onCancel?: () => void
}) {
  const toast = useToast()
  const { run, pending } = useMutate()
  const preset = card ? undefined : refMaps.paymentMethodById.get(familyOf ?? -1)
  // 가족카드는 본인 카드와 실적·혜택을 함께 쓴다. 본인 카드는 가족카드가 아닌 카드이고, 가족카드가 있는 카드는 가족카드가 될 수 없다
  const canBeFamily = kinds.some((k) => k === 'CREDIT_CARD' || k === 'CHECK_CARD')
  const familyCount = card ? familyCardsOf(refMaps, card.id).length : 0
  const parentChoices = refMaps.paymentMethods.filter(
    (p) => p.isCard && p.parentId == null && p.id !== card?.id && (!p.archived || p.id === card?.parentId),
  )
  const [family, setFamily] = useState(card ? card.parentId != null : preset != null)
  const [parentId, setParentId] = useState(numStr(card ? card.parentId : preset?.id))
  const parent = family ? refMaps.paymentMethodById.get(Number(parentId) || -1) : undefined
  const [name, setName] = useState(card?.name ?? (preset ? `${preset.name} 가족` : ''))
  const [kind, setKind] = useState<PaymentKind>(card?.kind ?? kinds[0])
  const [issuer, setIssuer] = useState(card?.issuer ?? preset?.issuer ?? '')
  const [last4, setLast4] = useState(card?.last4 ?? '')
  const [aliases, setAliases] = useState((card?.aliases ?? []).join(', '))
  const [accountId, setAccountId] = useState(numStr(card?.accountId))
  const [paymentDay, setPaymentDay] = useState(numStr(card?.paymentDay))
  const [closingDay, setClosingDay] = useState(numStr(card?.closingDay))
  const [closingOffset, setClosingOffset] = useState(card?.closingMonthOffset != null ? String(card.closingMonthOffset) : '-1')
  const [expiresOn, setExpiresOn] = useState(card?.expiresOn ?? '')
  const [memo, setMemo] = useState(card?.memo ?? '')
  // 카드 상품(전월실적 목표·구간·혜택·연회비)은 모든 계정이 함께 쓴다. 가족카드는 본인 카드의 상품
  const products = useGql<{ cardProducts: CardProduct[] }>(['cardProducts'], CARD_PRODUCTS_QUERY)
  const productList = products.data?.cardProducts ?? []
  const [productId, setProductId] = useState(numStr(card ? card.cardProductId : preset?.cardProductId))
  const [productFilter, setProductFilter] = useState('')
  const [editingProduct, setEditingProduct] = useState<'new' | CardProduct | null>(null)
  const selectedProduct = productList.find((p) => String(p.id) === productId)
  const norm = (t: string) => t.replace(/\s/g, '').toLowerCase()
  const visibleProducts = productList.filter((p) => String(p.id) === productId || norm(p.label).includes(norm(productFilter)))

  // 가족카드는 종류·결제 계좌·정산 일정·카드 상품이 본인 카드를 따른다
  const effectiveKind = family && parent ? parent.kind : kind
  const isCredit = !family && effectiveKind === 'CREDIT_CARD'
  const isCard = effectiveKind === 'CREDIT_CARD' || effectiveKind === 'CHECK_CARD'
  const ownSettings = isCard && !family
  const billingFilled = paymentDay !== '' && closingDay !== ''

  const chooseParent = (value: string) => {
    setParentId(value)
    const p = refMaps.paymentMethodById.get(Number(value) || -1)
    if (!p) return
    if (!issuer.trim()) setIssuer(p.issuer ?? '')
    if (!name.trim()) setName(`${p.name} 가족`)
    // 가족카드에서 풀 때 본인 카드의 상품을 그대로 쓰도록 미리 맞춰 둔다
    setProductId(numStr(p.cardProductId))
  }

  const toggleFamily = (on: boolean) => {
    setFamily(on)
    if (on && !parentId && parentChoices.length === 1) chooseParent(String(parentChoices[0].id))
  }

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
    if (family && !parent) {
      toast('가족카드의 본인 카드를 고르세요', 'error')
      return
    }
    const input = {
      parentId: family && parent ? parent.id : null,
      name: name.trim(),
      kind: effectiveKind,
      issuer: issuer || null,
      // 가족카드는 서버가 본인 카드의 상품으로 맞춘다
      cardProductId: ownSettings && productId ? Number(productId) : null,
      last4: last4 || null,
      aliases: aliases.split(',').map((a) => a.trim()).filter(Boolean),
      accountId: !family && accountId ? Number(accountId) : null,
      paymentDay: isCredit && billingFilled ? Number(paymentDay) : null,
      closingDay: isCredit && billingFilled ? Number(closingDay) : null,
      closingMonthOffset: isCredit && billingFilled ? Number(closingOffset) : null,
      expiresOn: expiresOn || null,
      memo: memo || null,
    }
    if (!input.name) {
      toast('이름을 입력하세요', 'error')
      return
    }
    if (isCredit && (paymentDay === '') !== (closingDay === '')) {
      toast('정산 일정은 결제일과 마감일을 모두 입력하세요', 'error')
      return
    }
    // 카드 상품이 바뀌면 이 카드 거래·반복 규칙에 고른 이전 상품의 혜택 표시를 서버가 지운다
    const nextProduct = family ? (parent?.cardProductId ?? null) : input.cardProductId
    if (card && card.cardProductId != null && nextProduct !== card.cardProductId) {
      const msg = family
        ? '가족카드로 바꾸면 본인 카드의 상품으로 실적·혜택을 함께 계산하고, 이 카드 거래에 고른 이전 상품의 혜택 표시가 지워집니다. 계속할까요?'
        : '카드 상품을 바꾸면 이 카드 거래·반복 규칙에 고른 이전 상품의 혜택 표시가 지워집니다. 계속할까요?'
      if (!window.confirm(msg)) return
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

  if (editingProduct) {
    const editing = editingProduct === 'new' ? undefined : editingProduct
    return (
      <div className="stack">
        <h3>{editing ? `카드 상품 수정 · ${editing.label}` : '새 카드 상품'}</h3>
        <CardProductForm
          product={editing}
          initialIssuer={issuer}
          onDone={(id) => {
            setProductId(String(id))
            setProductFilter('')
            setEditingProduct(null)
          }}
          onCancel={() => setEditingProduct(null)}
        />
      </div>
    )
  }

  return (
    <form className="stack" onSubmit={submit}>
      {canBeFamily && (
        <div className="row" style={{ flexWrap: 'wrap' }}>
          <label className="check">
            <input type="checkbox" checked={family} disabled={familyCount > 0} onChange={(e) => toggleFamily(e.target.checked)} />
            가족카드
          </label>
          <span className="small text-2">
            {familyCount > 0
              ? `가족카드 ${familyCount}장이 있는 본인 카드라 가족카드로 바꿀 수 없습니다`
              : family
                ? '결제 계좌·정산 일정은 본인 카드를 따르고, 전월실적·혜택은 본인 카드와 함께 계산합니다'
                : '본인 카드와 전월실적·혜택을 함께 쓰는 카드면 체크하세요'}
          </span>
        </div>
      )}
      <div className="form-grid">
        <label className="field">
          이름 *
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="예: MG+S" required />
        </label>
        {family ? (
          <label className="field">
            본인 카드 *
            <select className="input" value={parentId} onChange={(e) => chooseParent(e.target.value)} required>
              <option value="">선택</option>
              {parentChoices.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                  {p.archived ? ' (보관)' : ''}
                </option>
              ))}
            </select>
            <span className="hint">{parent ? PAYMENT_KIND_LABEL[parent.kind] : parentChoices.length === 0 ? '고를 수 있는 카드가 없습니다' : '실적·혜택을 함께 쓸 카드'}</span>
          </label>
        ) : (
          <label className="field">
            종류
            <select className="input" value={kind} onChange={(e) => setKind(e.target.value as PaymentKind)}>
              {kinds.map((k) => (
                <option key={k} value={k}>
                  {PAYMENT_KIND_LABEL[k]}
                </option>
              ))}
            </select>
          </label>
        )}
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
            카드번호 끝 4자리
            <input className="input" inputMode="numeric" maxLength={4} value={last4} onChange={(e) => setLast4(e.target.value.replace(/\D/g, ''))} />
            <span className="hint">승인 문자 자동 입력 시 카드 구분에 씀</span>
          </label>
        )}
        {!family && (
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
        )}
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

      {ownSettings && (
        <fieldset className="panel" style={{ padding: 14, border: '1px solid var(--border)' }}>
          <legend style={{ fontWeight: 650, padding: '0 6px' }}>카드 상품</legend>
          <div className="row" style={{ flexWrap: 'wrap', alignItems: 'end' }}>
            {productList.length > 8 && (
              <input className="input" placeholder="상품 검색" value={productFilter} onChange={(e) => setProductFilter(e.target.value)} style={{ width: 160 }} aria-label="카드 상품 검색" />
            )}
            <select className="input" value={productId} onChange={(e) => setProductId(e.target.value)} aria-label="카드 상품" style={{ minWidth: 220 }}>
              <option value="">선택 안 함 (실적·혜택 계산 안 함)</option>
              {visibleProducts.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
            <button type="button" className="btn" onClick={() => setEditingProduct('new')}>
              새 상품
            </button>
            {selectedProduct?.canEdit && (
              <button type="button" className="btn ghost" onClick={() => setEditingProduct(selectedProduct)}>
                상품 수정
              </button>
            )}
          </div>
          {selectedProduct ? (
            <ProductSummary product={selectedProduct} />
          ) : (
            <p className="small text-2" style={{ margin: '10px 0 0' }}>
              전월실적 목표·구간·혜택·연회비는 카드 상품에 있고 모든 계정이 함께 씁니다. 목록에 없으면 새 상품을 만드세요.
            </p>
          )}
        </fieldset>
      )}

      <div className="form-grid">
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

      {family && parent && (
        <div className="notice info">
          <span className="notice-icon">i</span>
          <span>
            {parent.name}의 결제 계좌·정산 일정({parent.billingConfigured ? `매월 ${parent.paymentDay}일 결제` : '미설정'})과 카드 상품
            {parent.productName ? `(${parent.productName})` : ''}을 따르고, 전월실적은 {parent.name} 이용과 합쳐 계산합니다. 거래에는 {parent.name}의 혜택
            {parent.benefits.length ? `(${parent.benefits.map((b) => b.name).join(', ')})` : ''}을 고릅니다.
          </span>
        </div>
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

/** 고른 카드 상품의 요약: 연회비·전월실적 목표·구간·혜택 */
function ProductSummary({ product }: { product: CardProduct }) {
  const tiers = [...product.tiers].sort((a, b) => a.minSpending - b.minSpending)
  return (
    <dl className="small text-2" style={{ margin: '10px 0 0', display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '4px 12px' }}>
      <dt>연회비</dt>
      <dd style={{ margin: 0 }}>{product.annualFee != null ? `${won(product.annualFee)}원` : '-'}</dd>
      <dt>전월실적</dt>
      <dd style={{ margin: 0 }}>
        {product.monthlyTarget != null ? `목표 ${won(product.monthlyTarget)}원` : '-'}
        {tiers.length > 0 && ` · 구간 ${tiers.map((t) => `${won(t.minSpending)}원↑ ${won(t.benefitLimit)}원`).join(', ')}`}
      </dd>
      <dt>혜택</dt>
      <dd style={{ margin: 0 }}>{product.benefits.length ? product.benefits.map((b) => b.name).join(', ') : '-'}</dd>
      {product.memo && (
        <>
          <dt>메모</dt>
          <dd style={{ margin: 0 }}>{product.memo}</dd>
        </>
      )}
      <dt>공유</dt>
      <dd style={{ margin: 0 }}>
        카드 {product.cardCount}장이 사용 중 · {product.canEdit ? (product.createdByMe ? '내가 만든 상품' : '관리자로 수정 가능') : '만든 사람만 수정'}
      </dd>
    </dl>
  )
}
