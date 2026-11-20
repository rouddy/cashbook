import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import { useGql, useMutate, useRefData } from '../api/hooks'
import { CARD_LIST_QUERY, CARDS_MONTHLY_QUERY, DELETE_PM, RESTORE_PM } from '../api/queries'
import type { CardMonthReport, PaymentMethod, StatementInfo } from '../api/types'
import CardForm from '../components/CardForm'
import { ErrorBox, Loading, PerformanceMeter, PlannedSwatch } from '../components/Common'
import Modal from '../components/Modal'
import { useToast } from '../components/Toast'
import { benefitTotalsText, hasBenefit, plannedPerformance } from '../lib/benefits'
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

  const reportOf = new Map((reports.data?.cardsMonthly ?? []).map((r) => [r.paymentMethodId, r]))
  const list = cards.data?.paymentMethods ?? []

  const remove = async (c: CardRow) => {
    const msg =
      c.transactionCount > 0
        ? `'${c.name}'에 거래 ${c.transactionCount}건이 있어 삭제 대신 보관합니다. 내역은 그대로 남고 목록과 입력 선택지에서 빠집니다. 계속할까요?`
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
                  {c.archived && <span className="badge">보관</span>}
                </div>
                {c.kind === 'CREDIT_CARD' &&
                  (c.billingConfigured ? (
                    <div className="small text-2">
                      매월 {c.paymentDay}일 결제 · {c.usagePeriodLabel} 이용
                    </div>
                  ) : (
                    <div className="small">
                      <span className="badge warn">! 정산 일정 미설정</span> <Link to={`/cards/${c.id}?edit=1`}>입력하기</Link>
                    </div>
                  ))}
                {next && (
                  <div className="row" style={{ alignItems: 'baseline' }}>
                    <span className="text-2 small">다음 결제 {shortDate(next.paymentDate)}</span>
                    <span className="spacer" />
                    <span style={{ fontWeight: 650 }}>{won(next.confirmedAmount)}원</span>
                    {next.plannedAmount !== 0 && <span className="small muted">+예정 {won(next.plannedAmount)}</span>}
                  </div>
                )}
                {r && (
                  <div>
                    <div className="row small" style={{ marginBottom: 4 }}>
                      <span className="text-2">이번 달 실적</span>
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
                    {(hasBenefit(r) || r.currentLimit != null) && (
                      <div className="row small" style={{ marginTop: 6 }}>
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
