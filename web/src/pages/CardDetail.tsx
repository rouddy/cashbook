import { useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom'

import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import { CARD_DETAIL_QUERY, DELETE_PM, RESTORE_PM, STATEMENT_LINES_QUERY, TRANSACTIONS_QUERY } from '../api/queries'
import type { CardMonthReport, PaymentMethod, StatementInfo, StatementLine, TransactionPage } from '../api/types'
import CardForm from '../components/CardForm'
import { ErrorBox, Loading, Meter, Panel } from '../components/Common'
import Modal from '../components/Modal'
import MonthPicker from '../components/MonthPicker'
import { useToast } from '../components/Toast'
import TransactionGrid, { PageTotals } from '../components/TransactionGrid'
import { benefitRateText, benefitTotalsText, benefitValueText, separateLimitLabel, txBenefitTexts } from '../lib/benefits'
import { longDate, PAYMENT_KIND_LABEL, shortDate, won } from '../lib/format'
import { defaultDateFor, localToday, monthLabel, useMonthParam } from '../lib/month'

type Detail = PaymentMethod & {
  transactionCount: number
  lastUsedOn: string | null
  account: { id: number; name: string } | null
  nextPayment: StatementInfo | null
  statements: StatementInfo[]
  monthReport: CardMonthReport
}

export default function CardDetail() {
  const { id } = useParams()
  const cardId = Number(id)
  const { ref, error: refError } = useRefData()
  const [month, setMonth] = useMonthParam()
  const [params, setParams] = useSearchParams()
  const editing = params.get('edit') === '1'
  const setEditing = (on: boolean) =>
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev)
        if (on) next.set('edit', '1')
        else next.delete('edit')
        return next
      },
      { replace: true },
    )
  const navigate = useNavigate()
  const toast = useToast()
  const { run, pending } = useMutate()
  const detail = useGql<{ paymentMethod: Detail | null }>(['card', cardId, month], CARD_DETAIL_QUERY, { id: cardId, month })

  if (refError) return <ErrorBox error={refError} />
  if (!ref || detail.isLoading) return <Loading />
  if (detail.error) return <ErrorBox error={detail.error} />
  const c = detail.data?.paymentMethod
  if (!c) return <div className="panel empty">카드를 찾을 수 없습니다. <Link to="/cards">카드 목록</Link></div>

  const remove = async () => {
    const msg =
      c.transactionCount > 0
        ? `거래 ${c.transactionCount}건이 있어 삭제 대신 보관합니다 (내역은 유지). 계속할까요?`
        : `'${c.name}'을(를) 삭제할까요?`
    if (!window.confirm(msg)) return
    const r = await run<{ deletePaymentMethod: { deleted: boolean } }>(DELETE_PM, { id: c.id })
    if (!r) return
    toast(r.deletePaymentMethod.deleted ? '삭제했습니다' : '보관했습니다')
    navigate('/cards')
  }

  return (
    <div className="stack">
      <div className="page-head">
        <Link to="/cards" className="btn sm ghost">
          ‹ 카드
        </Link>
        <span className="card-chip" aria-hidden />
        <h1>{c.name}</h1>
        <span className="badge">{PAYMENT_KIND_LABEL[c.kind]}</span>
        {c.archived && <span className="badge">보관</span>}
        <span className="spacer" />
        <button className="btn" onClick={() => setEditing(!editing)}>
          {editing ? '편집 닫기' : '편집'}
        </button>
        {c.archived ? (
          <button className="btn" disabled={pending} onClick={() => run(RESTORE_PM, { id: c.id })}>
            다시 사용
          </button>
        ) : (
          <button className="btn danger" disabled={pending} onClick={remove}>
            삭제
          </button>
        )}
      </div>

      {editing && (
        <Panel title="카드 정보 편집">
          <CardForm card={c} refMaps={ref} onDone={() => setEditing(false)} onCancel={() => setEditing(false)} />
        </Panel>
      )}

      <div className="grid-2">
        <Panel title="카드 정보">
          <dl className="kv">
            <dt>카드사</dt>
            <dd>{c.issuer ?? '-'}</dd>
            <dt>상품명</dt>
            <dd>{c.productName ?? '-'}</dd>
            <dt>카드번호</dt>
            <dd>{c.last4 ? `•••• ${c.last4}` : '-'}</dd>
            <dt>결제 계좌</dt>
            <dd>{c.account ? <Link to="/accounts">{c.account.name}</Link> : '-'}</dd>
            {c.kind === 'CREDIT_CARD' && (
              <>
                <dt>정산 일정</dt>
                <dd>
                  {c.billingConfigured ? (
                    <>
                      매월 {c.paymentDay}일 결제 · {c.usagePeriodLabel} 이용
                    </>
                  ) : (
                    <span>
                      <span className="badge warn">! 미설정</span>{' '}
                      <button className="btn sm" onClick={() => setEditing(true)}>
                        입력하기
                      </button>
                    </span>
                  )}
                </dd>
              </>
            )}
            <dt>전월실적 목표</dt>
            <dd>{c.monthlyTarget ? `${won(c.monthlyTarget)}원` : '-'}</dd>
            <dt>연회비</dt>
            <dd>{c.annualFee ? `${won(c.annualFee)}원` : '-'}</dd>
            <dt>유효기간</dt>
            <dd>{c.expiresOn ?? '-'}</dd>
            <dt>자동입력 별칭</dt>
            <dd>{c.aliases.length ? c.aliases.join(', ') : '-'}</dd>
            <dt>거래</dt>
            <dd>
              {won(c.transactionCount)}건{c.lastUsedOn ? ` · 최근 ${longDate(c.lastUsedOn)}` : ''}
            </dd>
            {c.memo && (
              <>
                <dt>메모</dt>
                <dd>{c.memo}</dd>
              </>
            )}
          </dl>
        </Panel>
        <StatementsPanel card={c} />
      </div>

      <div className="filters">
        <MonthPicker month={month} onChange={setMonth} />
        <Link className="btn sm ghost" to={`/card-transactions?card=${c.id}&month=${month}`}>
          카드별 내역에서 보기
        </Link>
      </div>
      <ReportPanel report={c.monthReport} card={c} />
      <CardMonthTransactions card={c} month={month} refMaps={ref} report={c.monthReport} />
    </div>
  )
}

function StatementsPanel({ card }: { card: Detail }) {
  const [open, setOpen] = useState<StatementInfo | null>(null)
  if (card.kind !== 'CREDIT_CARD') {
    return (
      <Panel title="결제">
        <p className="text-2">{card.kind === 'CHECK_CARD' ? '체크카드는 결제 시 연결 계좌에서 바로 빠져나갑니다.' : '카드가 아닌 결제수단입니다.'}</p>
      </Panel>
    )
  }
  if (!card.billingConfigured) {
    return (
      <Panel title="결제 예정 금액">
        <div className="notice">
          <span className="notice-icon">!</span>
          <span>정산 일정(결제일·이용기간)을 입력하면 청구서별 결제 예정 금액을 계산합니다.</span>
        </div>
      </Panel>
    )
  }
  const next = card.nextPayment
  return (
    <Panel title="결제 예정 금액" sub="청구서(결제월)별 · 할부는 회차 금액으로 나눠 계산" flush>
      {next && (
        <div style={{ padding: '0 16px 12px' }}>
          <div className="text-2 small">다음 결제 {longDate(next.paymentDate)}</div>
          <div className="hero" style={{ fontSize: 32 }}>
            {won(next.confirmedAmount)}
            <span style={{ fontSize: 16 }}>원</span>
          </div>
          <div className="small text-2">
            이용기간 {shortDate(next.periodStart, false)} ~ {shortDate(next.periodEnd, false)}
            {next.closed ? ' (마감)' : ' (진행 중 · 금액이 늘어날 수 있음)'}
            {next.plannedAmount !== 0 && ` · 예정 ${won(next.plannedAmount)}원 별도`}
          </div>
        </div>
      )}
      <table className="table">
        <thead>
          <tr>
            <th>결제일</th>
            <th>이용기간</th>
            <th className="num">확정</th>
            <th className="num">예정</th>
            <th>상태</th>
          </tr>
        </thead>
        <tbody>
          {card.statements.map((s) => (
            <tr key={s.month} className="clickable" onClick={() => setOpen(s)}>
              <td className="nowrap">{shortDate(s.paymentDate)}</td>
              <td className="nowrap small">
                {shortDate(s.periodStart, false)}~{shortDate(s.periodEnd, false)}
              </td>
              <td className="num">{won(s.confirmedAmount)}</td>
              <td className="num text-2">{s.plannedAmount ? won(s.plannedAmount) : '-'}</td>
              <td>
                <StatementBadge s={s} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {open && <StatementModal card={card} statement={open} onClose={() => setOpen(null)} />}
    </Panel>
  )
}

function StatementBadge({ s }: { s: StatementInfo }) {
  if (s.paid) return <span className="badge">결제됨</span>
  if (s.closed) return <span className="badge planned">결제 예정</span>
  if (s.periodStart > localToday()) return <span className="badge">이용 전</span>
  return <span className="badge warn">이용 중</span>
}

function StatementModal({ card, statement, onClose }: { card: Detail; statement: StatementInfo; onClose: () => void }) {
  const q = useGql<{ statementLines: StatementLine[] }>(['statementLines', card.id, statement.month], STATEMENT_LINES_QUERY, {
    id: card.id,
    month: statement.month,
  })
  const lines = q.data?.statementLines ?? []
  return (
    <Modal title={`${card.name} · ${shortDate(statement.paymentDate)} 결제분`} onClose={onClose}>
      <p className="text-2 small" style={{ marginTop: 0 }}>
        이용기간 {longDate(statement.periodStart)} ~ {longDate(statement.periodEnd)}
      </p>
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorBox error={q.error} />
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>이용일</th>
              <th>내용</th>
              <th className="num">청구 금액</th>
            </tr>
          </thead>
          <tbody>
            {lines.map((l) => (
              <tr key={`${l.transaction.id}-${l.installmentNo}`}>
                <td className="nowrap">{shortDate(l.transaction.date)}</td>
                <td>
                  {l.transaction.description}
                  {l.installmentMonths > 1 && (
                    <span className="badge" style={{ marginLeft: 6 }}>
                      할부 {l.installmentNo}/{l.installmentMonths}
                    </span>
                  )}
                  {l.planned && (
                    <span className="badge planned" style={{ marginLeft: 6 }}>
                      예정
                    </span>
                  )}
                </td>
                <td className="num">{won(l.amount)}</td>
              </tr>
            ))}
            {lines.length === 0 && (
              <tr>
                <td colSpan={3} className="empty">
                  이 청구서에 들어간 거래가 없습니다
                </td>
              </tr>
            )}
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={2}>합계 ({lines.length}건)</td>
              <td className="num">{won(lines.reduce((a, l) => a + l.amount, 0))}</td>
            </tr>
          </tfoot>
        </table>
      )}
    </Modal>
  )
}

export function ReportPanel({ report: r, card }: { report: CardMonthReport; card: PaymentMethod }) {
  const tiers = [...card.tiers].sort((a, b) => a.minSpending - b.minSpending)
  return (
    <Panel title={`${monthLabel(r.month)} 실적 · 혜택`} sub="실적은 이용일 기준 달력월 합계 (실적 제외 표시·실적 제외 혜택 거래는 빠짐)">
      <div className="kpis">
        <div>
          <div className="stat-label">이번 달 실적</div>
          <div className="stat-value">{won(r.performance)}원</div>
          {r.target ? (
            <>
              <div style={{ margin: '6px 0' }}>
                <Meter goal value={r.performance} max={r.target} label="전월실적 목표 대비" />
              </div>
              <div className="small text-2">
                목표 {won(r.target)}원 ·{' '}
                {r.remainingToTarget ? <b>{won(r.remainingToTarget)}원 더 쓰면 달성</b> : <span className="good">✓ 달성</span>}
              </div>
            </>
          ) : (
            <div className="small text-2">목표 미설정</div>
          )}
          {r.plannedSpending !== 0 && <div className="small text-2">예정 포함 전망 {won(r.projectedPerformance)}원</div>}
        </div>
        <div>
          <div className="stat-label">이번 달 혜택</div>
          <div className="stat-value">{benefitTotalsText(r)}</div>
          <div className="small text-2">
            {r.currentLimit != null ? (
              <>
                지난달 실적 {won(r.previousPerformance)}원 → 한도 {won(r.currentLimit)}원
                {r.benefitRemaining != null && ` · 남은 한도 ${won(r.benefitRemaining)}원`}
                {r.benefitTotal > r.limitUsed && ` · 한도와 무관한 혜택 ${won(r.benefitTotal - r.limitUsed)}원`}
              </>
            ) : (
              `지난달 실적 ${won(r.previousPerformance)}원 · 통합 한도 없음`
            )}
          </div>
          {r.currentLimit != null && r.currentLimit > 0 && (
            <div style={{ marginTop: 6 }}>
              <Meter value={r.limitUsed} max={r.currentLimit} label="혜택 한도 사용" />
            </div>
          )}
        </div>
        {tiers.length > 0 && (
          <div>
            <div className="stat-label">다음 달 혜택 구간</div>
            <div className="stat-value">{r.achievedTier ? `${won(r.achievedTier.benefitLimit)}원` : '구간 미달'}</div>
            <div className="small text-2">
              {r.nextTier ? (
                <>
                  {won(r.remainingToNextTier)}원 더 쓰면 {won(r.nextTier.minSpending)}원 구간 (한도 {won(r.nextTier.benefitLimit)}원)
                </>
              ) : (
                '최고 구간 달성'
              )}
            </div>
            <div className="small muted" style={{ marginTop: 4 }}>
              {tiers.map((t) => `${won(t.minSpending)}↑ ${won(t.benefitLimit)}`).join(' · ')}
            </div>
          </div>
        )}
      </div>
      {r.benefits.length > 0 && (
        <div className="table-wrap" style={{ marginTop: 14 }}>
          <table className="table">
            <thead>
              <tr>
                <th>혜택</th>
                <th className="num">혜택률</th>
                <th className="num">대상 이용액</th>
                <th className="num">혜택액</th>
                <th className="num">남은 혜택</th>
                <th className="num">한도까지 더 쓸 금액</th>
              </tr>
            </thead>
            <tbody>
              {r.benefits.map((b) => (
                <tr key={b.cardBenefitId}>
                  <td>
                    {b.name}
                    {separateLimitLabel(b) && (
                      <span className="badge" style={{ marginLeft: 6 }}>
                        {separateLimitLabel(b)}
                      </span>
                    )}
                    {b.stackable && (
                      <span className="badge" style={{ marginLeft: 6 }} title="다른 혜택과 함께 받는 혜택">
                        중복 적용
                      </span>
                    )}
                  </td>
                  <td className="num">{benefitRateText(b)}</td>
                  <td className="num">
                    {won(b.spending)} <span className="muted small">({b.count}건)</span>
                  </td>
                  <td className="num">
                    {benefitValueText(b.kind, b.amount)}
                    {b.raw !== b.amount && <div className="small muted">한도 전 {benefitValueText(b.kind, b.raw)}</div>}
                  </td>
                  <td className="num">{b.remaining == null ? '제한 없음' : benefitValueText(b.kind, b.remaining)}</td>
                  <td className="num">{b.spendToMax == null ? '-' : b.spendToMax === 0 ? '한도 소진' : won(b.spendToMax)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  )
}

function CardMonthTransactions({ card, month, refMaps, report }: { card: PaymentMethod; month: string; refMaps: RefMaps; report: CardMonthReport }) {
  const filter = { month, paymentMethodId: card.id, statuses: ['CONFIRMED', 'PLANNED'] }
  const q = useGql<{ transactions: TransactionPage }>(['transactions', filter], TRANSACTIONS_QUERY, { filter, sort: 'DATE_ASC', limit: 2000 })
  const benefitAmounts = txBenefitTexts(report)
  const page = q.data?.transactions
  return (
    <Panel title={`${monthLabel(month)} ${card.name} 거래`} sub="혜택 칸을 눌러 거래별 혜택 항목을 지정하세요" flush>
      {q.error ? (
        <ErrorBox error={q.error} />
      ) : !page ? (
        <Loading />
      ) : (
        <TransactionGrid
          rows={page.items}
          refMaps={refMaps}
          columns={['date', 'amount', 'category', 'description', 'benefits', 'benefitAmount', 'member', 'memo']}
          benefitAmounts={benefitAmounts}
          newRow={{ date: defaultDateFor(month), ledgerId: refMaps.defaultLedger?.id ?? null, paymentMethodId: card.id }}
          footer={<PageTotals page={page} />}
        />
      )}
    </Panel>
  )
}
