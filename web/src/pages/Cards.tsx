import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import { useGql, useMutate, useRefData } from '../api/hooks'
import { CARD_LIST_QUERY, CARD_PRODUCTS_QUERY, CARDS_MONTHLY_QUERY, DELETE_CARD_PRODUCT, DELETE_PM, RESTORE_PM } from '../api/queries'
import type { CardMonthReport, CardProduct, PaymentMethod, StatementInfo } from '../api/types'
import CardForm from '../components/CardForm'
import CardProductForm from '../components/CardProductForm'
import { ErrorBox, Loading, Panel, PerformanceMeter, PlannedSwatch } from '../components/Common'
import Modal from '../components/Modal'
import { useToast } from '../components/Toast'
import { benefitTotalsText, hasBenefit, plannedPerformance, remainingSummary } from '../lib/benefits'
import { familyCardsOf, withFamilyOrder } from '../lib/cards'
import { PAYMENT_KIND_LABEL, shortDate, signedWon, won } from '../lib/format'
import { currentMonth } from '../lib/month'

type CardRow = PaymentMethod & {
  transactionCount: number
  lastUsedOn: string | null
  account: { id: number; name: string } | null
  nextPayment: StatementInfo | null
}

export default function Cards() {
  const { ref, error } = useRefData()
  const navigate = useNavigate()
  const toast = useToast()
  const [showArchived, setShowArchived] = useState(false)
  const [adding, setAdding] = useState(false)
  const month = currentMonth()
  const cards = useGql<{ paymentMethods: CardRow[] }>(['cards', showArchived], CARD_LIST_QUERY, { includeArchived: showArchived })
  const reports = useGql<{ cardsMonthly: CardMonthReport[] }>(['cardsMonthly', month], CARDS_MONTHLY_QUERY, { month })
  const { run, pending } = useMutate()

  if (error) return <ErrorBox error={error} />
  if (!ref) return <Loading />

  // 가족카드는 실적이 본인 카드 리포트에 합쳐져 따로 오지 않는다
  const reportOf = new Map((reports.data?.cardsMonthly ?? []).map((r) => [r.paymentMethodId, r]))
  const list = withFamilyOrder(cards.data?.paymentMethods ?? [], (c) => c)

  const remove = async (c: CardRow) => {
    const family = familyCardsOf(ref, c.id).filter((f) => !f.archived)
    const familyNote = family.length ? ` 가족카드 ${family.map((f) => f.name).join(', ')}도 함께 보관합니다.` : ''
    const msg =
      c.transactionCount > 0
        ? `'${c.name}'에 거래 ${c.transactionCount}건이 있어 삭제 대신 보관합니다. 내역은 그대로 남고 목록과 입력 선택지에서 빠집니다.${familyNote} 계속할까요?`
        : family.length
          ? `'${c.name}'에 가족카드가 있어 삭제 대신 보관합니다.${familyNote} 계속할까요?`
          : `'${c.name}'을(를) 삭제할까요?`
    if (!window.confirm(msg)) return
    const r = await run<{ deletePaymentMethod: { deleted: boolean; archived: boolean } }>(DELETE_PM, { id: c.id })
    if (!r) return
    toast(r.deletePaymentMethod.archived ? `${c.name}: 보관했습니다` : `${c.name}: 삭제했습니다`)
  }

  return (
    <div className="stack">
      <div className="page-head">
        <h1>카드</h1>
        <span className="spacer" />
        <label className="check">
          <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
          보관된 카드 보기
        </label>
        <button className="btn primary" onClick={() => setAdding(true)}>
          + 카드 추가
        </button>
      </div>
      {cards.error && <ErrorBox error={cards.error} />}
      {!cards.data ? (
        <Loading />
      ) : list.length === 0 ? (
        <div className="panel empty">등록된 카드가 없습니다</div>
      ) : (
        <div className="grid-cards">
          {list.map((c) => {
            const r = reportOf.get(c.id)
            const next = c.nextPayment
            const parent = c.parentId != null ? ref.paymentMethodById.get(c.parentId) : undefined
            const familyCount = familyCardsOf(ref, c.id).filter((f) => !f.archived).length
            const remaining = r ? remainingSummary(r) : null
            return (
              <article key={c.id} className="panel card-tile" style={c.archived ? { opacity: 0.65 } : undefined}>
                <div className="row">
                  <span className="card-chip" aria-hidden />
                  <div style={{ minWidth: 0 }}>
                    <Link to={`/cards/${c.id}`} className="card-name">
                      {c.name}
                    </Link>
                    <div className="small text-2">
                      {[c.issuer, c.productName, c.last4 && `•••• ${c.last4}`].filter(Boolean).join(' · ') || PAYMENT_KIND_LABEL[c.kind]}
                    </div>
                  </div>
                  <span className="spacer" />
                  <span className="badge">{PAYMENT_KIND_LABEL[c.kind]}</span>
                  {parent && <span className="badge">가족카드</span>}
                  {c.archived && <span className="badge">보관</span>}
                </div>
                {c.kind === 'CREDIT_CARD' &&
                  (c.billingConfigured ? (
                    <div className="small text-2">
                      매월 {c.paymentDay}일 결제 · {c.usagePeriodLabel} 이용
                    </div>
                  ) : (
                    <div className="small">
                      <span className="badge warn">! 정산 일정 미설정</span>{' '}
                      <Link to={`/cards/${parent?.id ?? c.id}?edit=1`}>{parent ? '본인 카드에서 입력' : '입력하기'}</Link>
                    </div>
                  ))}
                {next && (
                  <div className="row" style={{ alignItems: 'baseline' }}>
                    <span className="text-2 small">
                      다음 결제 {shortDate(next.paymentDate)}
                      {parent ? ' · 본인 카드에 합산' : familyCount > 0 ? ' · 가족카드 포함' : ''}
                    </span>
                    <span className="spacer" />
                    {/* 가족카드는 본인 카드 청구서에 들어가는 이용분이라 굵게 강조하지 않는다 */}
                    <span style={parent ? undefined : { fontWeight: 650 }}>{won(next.confirmedAmount)}원</span>
                    {next.plannedAmount !== 0 && <span className="small muted">+예정 {won(next.plannedAmount)}</span>}
                  </div>
                )}
                {parent && (
                  <div className="row small">
                    <span className="text-2">
                      본인 카드 <Link to={`/cards/${parent.id}`}>{parent.name}</Link>의 실적·혜택을 함께 씀
                    </span>
                    <span className="spacer" />
                    {c.annualFee ? <span className="muted">연회비 {won(c.annualFee)}원</span> : null}
                  </div>
                )}
                {r && (
                  <div>
                    <div className="row small" style={{ marginBottom: 4 }}>
                      <span className="text-2">
                        이번 달 실적{familyCount > 0 && <span className="muted"> · 가족카드 {familyCount}장 포함</span>}
                      </span>
                      <span className="spacer" />
                      <span>
                        {won(r.performance)}
                        {r.target ? <span className="muted"> / {won(r.target)}</span> : null}
                      </span>
                    </div>
                    {plannedPerformance(r) !== 0 && (
                      <div className="row small" style={{ marginBottom: 4 }}>
                        <span className="spacer" />
                        <span className="muted">
                          {plannedPerformance(r) > 0 && <PlannedSwatch />}
                          예정 {signedWon(plannedPerformance(r))} · 포함 {won(r.projectedPerformance)}
                        </span>
                      </div>
                    )}
                    {r.target ? (
                      <PerformanceMeter report={r} label={`${c.name} 전월실적 목표 대비`} />
                    ) : null}
                    {(hasBenefit(r) || r.currentLimit != null || remaining) && (
                      <div style={{ marginTop: 6 }}>
                        {(hasBenefit(r) || r.currentLimit != null) && (
                          <div className="row small">
                            <span className="text-2">혜택</span>
                            <span className="spacer" />
                            <span>
                              {benefitTotalsText(r)}
                              {r.currentLimit != null && (
                                <span className="muted">
                                  {' '}
                                  · 한도 {won(r.limitUsed)} / {won(r.currentLimit)}
                                </span>
                              )}
                            </span>
                          </div>
                        )}
                        {remaining && (
                          <div className="row small">
                            <span className="text-2">남은 혜택</span>
                            <span className="spacer" />
                            <span>{remaining}</span>
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                )}
                <div className="row small text-2">
                  {c.account ? <span>출금 {c.account.name}</span> : <span className="muted">결제 계좌 미지정</span>}
                  <span className="spacer" />
                  <span>{c.lastUsedOn ? `최근 ${shortDate(c.lastUsedOn, false)}` : '사용 내역 없음'}</span>
                </div>
                <div className="row">
                  <button className="btn sm" onClick={() => navigate(`/cards/${c.id}`)}>
                    상세
                  </button>
                  <button className="btn sm" onClick={() => navigate(`/card-transactions?card=${c.id}`)}>
                    내역
                  </button>
                  <span className="spacer" />
                  {c.archived ? (
                    <button className="btn sm" disabled={pending} onClick={() => run(RESTORE_PM, { id: c.id })}>
                      다시 사용
                    </button>
                  ) : (
                    <button className="btn sm ghost danger" disabled={pending} onClick={() => remove(c)}>
                      삭제
                    </button>
                  )}
                </div>
              </article>
            )
          })}
        </div>
      )}
      <CardProducts />
      {adding && (
        <Modal title="카드 추가" onClose={() => setAdding(false)} width={860}>
          <CardForm
            refMaps={ref}
            onDone={(id) => {
              setAdding(false)
              navigate(`/cards/${id}`)
            }}
            onCancel={() => setAdding(false)}
          />
        </Modal>
      )}
    </div>
  )
}

/** 카드 상품 목록 (모든 계정 공용). 만든 계정과 관리자는 고치거나 (쓰는 카드가 없으면) 지울 수 있다 */
function CardProducts() {
  const products = useGql<{ cardProducts: CardProduct[] }>(['cardProducts'], CARD_PRODUCTS_QUERY)
  const { run, pending } = useMutate()
  const toast = useToast()
  const [editing, setEditing] = useState<'new' | CardProduct | null>(null)
  const list = products.data?.cardProducts ?? []
  const editingProduct = editing === 'new' ? undefined : (editing ?? undefined)
  return (
    <Panel
      title="카드 상품"
      sub="전월실적 목표·구간·혜택·연회비. 모든 계정이 함께 쓰고, 만든 사람과 관리자만 고칩니다. 카드 편집에서 상품을 고르세요."
      actions={
        <button className="btn sm" onClick={() => setEditing('new')}>
          + 새 상품
        </button>
      }
      flush
    >
      {products.error ? (
        <ErrorBox error={products.error} />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>상품</th>
                <th className="num">연회비</th>
                <th className="num">전월실적 목표</th>
                <th>혜택</th>
                <th className="num">사용 카드</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {list.map((p) => (
                <tr key={p.id}>
                  <td>
                    <b>{p.label}</b> {p.createdByMe && <span className="badge planned">내 상품</span>}
                    {p.memo && <div className="small muted">{p.memo}</div>}
                  </td>
                  <td className="num">{p.annualFee != null ? `${won(p.annualFee)}원` : '-'}</td>
                  <td className="num">{p.monthlyTarget != null ? `${won(p.monthlyTarget)}원` : '-'}</td>
                  <td className="small">{p.benefits.length ? p.benefits.map((b) => b.name).join(', ') : <span className="muted">없음</span>}</td>
                  <td className="num">{p.cardCount}</td>
                  <td className="nowrap" style={{ textAlign: 'right' }}>
                    {p.canEdit && (
                      <>
                        <button className="btn sm ghost" onClick={() => setEditing(p)}>
                          수정
                        </button>
                        <button
                          className="btn sm ghost danger"
                          disabled={pending || p.cardCount > 0}
                          title={p.cardCount > 0 ? '이 상품을 쓰는 카드가 있어 지울 수 없습니다' : undefined}
                          onClick={async () => {
                            if (!window.confirm(`'${p.label}' 카드 상품을 삭제할까요?`)) return
                            if (await run(DELETE_CARD_PRODUCT, { id: p.id })) toast('삭제했습니다')
                          }}
                        >
                          삭제
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
              {products.data && list.length === 0 && (
                <tr>
                  <td colSpan={6} className="empty">
                    등록된 카드 상품이 없습니다
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
      {editing && (
        <Modal title={editingProduct ? `카드 상품 수정 · ${editingProduct.label}` : '새 카드 상품'} onClose={() => setEditing(null)} width={860}>
          <CardProductForm product={editingProduct} onDone={() => setEditing(null)} onCancel={() => setEditing(null)} />
        </Modal>
      )}
    </Panel>
  )
}
