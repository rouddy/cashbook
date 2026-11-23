import { Link } from 'react-router-dom'

import { useGql, useRefData, type RefMaps } from '../api/hooks'
import { CARDS_MONTHLY_QUERY, STATEMENT_LINES_QUERY, TRANSACTIONS_QUERY } from '../api/queries'
import type { CardMonthReport, PaymentMethod, StatementLine, TransactionPage } from '../api/types'
import { ErrorBox, Loading, Panel, PerformanceMeter, PlannedSwatch } from '../components/Common'
import MonthPicker from '../components/MonthPicker'
import { SelectionActions } from '../components/DeleteTransactions'
import type { SuggestScope } from '../components/DescriptionInput'
import TransactionGrid, { PageTotals } from '../components/TransactionGrid'
import { benefitTotalsText, hasBenefit, plannedPerformance, txBenefitTexts } from '../lib/benefits'
import { familyReportNote, familyStatementNote, withFamilyOrder } from '../lib/cards'
import { shortDate, signedWon, won } from '../lib/format'
import { defaultDateFor, monthLabel, useMonthParam, useQueryParam } from '../lib/month'
import { useTxSelection } from '../lib/selection'
import { ReportPanel } from './CardDetail'

export default function CardTransactions() {
  const { ref, error } = useRefData()
  const [month, setMonth] = useMonthParam()
  const [cardParam, setCard] = useQueryParam('card', '')
  const [mode, setMode] = useQueryParam('mode', 'usage')
  // 가족카드도 이용금액을 따로 보도록 함께 받는다 (실적·혜택은 본인 카드와 같은 값)
  const reports = useGql<{ cardsMonthly: CardMonthReport[] }>(['cardsMonthly', month, 'family'], CARDS_MONTHLY_QUERY, { month, includeFamily: true })

  if (error) return <ErrorBox error={error} />
  if (!ref) return <Loading />
  const list = withFamilyOrder(reports.data?.cardsMonthly ?? [], (r) => ref.paymentMethodById.get(r.paymentMethodId))
  const selectedId = cardParam ? Number(cardParam) : list.find((r) => r.spending !== 0)?.paymentMethodId ?? list[0]?.paymentMethodId
  const selected = list.find((r) => r.paymentMethodId === selectedId)
  const card = selectedId ? ref.paymentMethodById.get(selectedId) : undefined
  const total = list.reduce((a, r) => a + r.spending, 0)

  return (
    <div className="stack">
      <div className="page-head">
        <h1>카드별 지출내역</h1>
        <div className="filters">
          <MonthPicker month={month} onChange={setMonth} />
          <div className="segmented" role="group" aria-label="기준">
            <button className={mode === 'usage' ? 'on' : ''} onClick={() => setMode('usage')}>
              이용월 기준
            </button>
            <button className={mode === 'statement' ? 'on' : ''} onClick={() => setMode('statement')}>
              결제월 기준
            </button>
          </div>
        </div>
      </div>
      {reports.error && <ErrorBox error={reports.error} />}
      <div className="grid-2" style={{ gridTemplateColumns: '280px minmax(0, 1fr)', alignItems: 'start' }}>
        <Panel title={`${monthLabel(month)} 카드 이용`} sub={`합계 ${won(total)}원`} flush>
          {!reports.data ? (
            <Loading />
          ) : (
            <div className="side-list" style={{ padding: '0 6px 8px' }}>
              {list.map((r) => {
                const pm = ref.paymentMethodById.get(r.paymentMethodId)
                const isFamily = pm?.parentId != null
                return (
                  <button
                    key={r.paymentMethodId}
                    className={r.paymentMethodId === selectedId ? 'on' : ''}
                    onClick={() => setCard(String(r.paymentMethodId))}
                    style={isFamily ? { paddingLeft: 26 } : undefined}
                  >
                    <div className="row">
                      <b>{pm?.name}</b>
                      {isFamily && <span className="badge">가족카드</span>}
                      {pm?.archived && <span className="badge">보관</span>}
                      <span className="spacer" />
                      <span className="num">{won(r.spending)}</span>
                    </div>
                    {isFamily ? (
                      <div className="small muted">실적·혜택은 본인 카드에 합산</div>
                    ) : r.target ? (
                      <div style={{ marginTop: 6 }}>
                        <PerformanceMeter report={r} label={`${pm?.name} 실적`} />
                        <div className="small text-2" style={{ marginTop: 2 }}>
                          실적 {won(r.performance)} / {won(r.target)}
                          {r.remainingToTarget ? ` · ${won(r.remainingToTarget)} 남음` : ' · 달성'}
                        </div>
                        {plannedPerformance(r) !== 0 && (
                          <div className="small muted">
                            {plannedPerformance(r) > 0 && <PlannedSwatch />}
                            예정 {signedWon(plannedPerformance(r))} · 포함 {won(r.projectedPerformance)}
                          </div>
                        )}
                      </div>
                    ) : (
                      <div className="small muted">실적 {won(r.performance)}</div>
                    )}
                    {!isFamily && (hasBenefit(r) || r.currentLimit) && (
                      <div className="small text-2">
                        혜택 {benefitTotalsText(r)}
                        {r.currentLimit != null && ` · 한도 ${won(r.limitUsed)} / ${won(r.currentLimit)}`}
                      </div>
                    )}
                  </button>
                )
              })}
            </div>
          )}
        </Panel>
        <div className="stack">
          {!selected || !card ? (
            <div className="panel empty">카드를 고르세요</div>
          ) : (
            <>
              <div className="row">
                <h2>{card.name}</h2>
                <Link to={`/cards/${card.id}?month=${month}`} className="btn sm ghost">
                  카드 상세
                </Link>
              </div>
              {mode === 'usage' ? (
                <>
                  <ReportPanel report={selected} card={card} note={familyReportNote(card, ref)} />
                  <UsageTransactions card={card} month={month} refMaps={ref} report={selected} />
                </>
              ) : (
                <StatementView card={card} month={month} refMaps={ref} />
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}

/** 내용 자동완성: 그 날짜(오늘보다 늦으면 오늘)부터 3개월 전 이후 내역만, 같은 내용이라도 항목이 다르면 따로 */
const CARD_SUGGEST_SCOPE: SuggestScope = { recentMonths: 3, byCategory: true }

function UsageTransactions({ card, month, refMaps, report }: { card: PaymentMethod; month: string; refMaps: RefMaps; report: CardMonthReport }) {
  const filter = { month, paymentMethodId: card.id, statuses: ['CONFIRMED', 'PLANNED'] }
  const q = useGql<{ transactions: TransactionPage }>(['transactions', filter], TRANSACTIONS_QUERY, { filter, sort: 'DATE_ASC', limit: 2000 })
  const page = q.data?.transactions
  const sel = useTxSelection(page?.items)
  return (
    <Panel title={`${monthLabel(month)} 이용 내역`} actions={<SelectionActions sel={sel} refMaps={refMaps} />} flush>
      {q.error ? (
        <ErrorBox error={q.error} />
      ) : !page ? (
        <Loading />
      ) : (
        <TransactionGrid
          rows={page.items}
          refMaps={refMaps}
          columns={['date', 'amount', 'description', 'ledger', 'category', 'benefits', 'benefitAmount', 'memo', 'excludePerformance']}
          benefitAmounts={txBenefitTexts(report)}
          newRow={{ date: defaultDateFor(month), ledgerId: refMaps.defaultLedger?.id ?? null, paymentMethodId: card.id }}
          selection={sel.grid}
          suggestScope={CARD_SUGGEST_SCOPE}
          footer={<PageTotals page={page} extra={<span>혜택 {benefitTotalsText(report)}</span>} />}
        />
      )}
    </Panel>
  )
}

function StatementView({ card, month, refMaps }: { card: PaymentMethod; month: string; refMaps: RefMaps }) {
  const enabled = card.kind === 'CREDIT_CARD' && card.billingConfigured
  const q = useGql<{ statementLines: StatementLine[] }>(['statementLines', card.id, month], STATEMENT_LINES_QUERY, { id: card.id, month }, { enabled })
  if (!enabled) {
    return (
      <div className="notice">
        <span className="notice-icon">!</span>
        <span>
          {card.kind === 'CREDIT_CARD' ? (
            <>
              정산 일정이 없어 결제월 기준으로 묶을 수 없습니다. <Link to={`/cards/${card.id}?edit=1`}>정산 일정 입력</Link>
            </>
          ) : (
            '체크카드/현금은 이용일에 바로 결제됩니다. 이용월 기준으로 보세요.'
          )}
        </span>
      </div>
    )
  }
  const lines = q.data?.statementLines ?? []
  const confirmed = lines.filter((l) => !l.planned).reduce((a, l) => a + l.amount, 0)
  const planned = lines.filter((l) => l.planned).reduce((a, l) => a + l.amount, 0)
  const note = familyStatementNote(card, refMaps)
  return (
    <Panel
      title={`${monthLabel(month)} 결제분`}
      sub={
        <>
          {card.usagePeriodLabel} 이용분이 매월 {card.paymentDay}일에 결제됩니다
          {note && <div>{note}</div>}
        </>
      }
      flush
    >
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
              <th>항목</th>
              <th className="num">청구 금액</th>
            </tr>
          </thead>
          <tbody>
            {lines.map((l) => (
              <tr key={`${l.transaction.id}-${l.installmentNo}`}>
                <td className="nowrap">{shortDate(l.transaction.date)}</td>
                <td>
                  {l.transaction.description}
                  {l.transaction.paymentMethodId !== card.id && (
                    <span className="badge" style={{ marginLeft: 6 }} title="가족카드 이용">
                      {refMaps.paymentMethodById.get(l.transaction.paymentMethodId ?? -1)?.name}
                    </span>
                  )}
                  {l.installmentMonths > 1 && (
                    <span className="badge" style={{ marginLeft: 6 }}>
                      {l.installmentNo}/{l.installmentMonths}회
                    </span>
                  )}
                  {l.planned && (
                    <span className="badge planned" style={{ marginLeft: 6 }}>
                      예정
                    </span>
                  )}
                </td>
                <td className="text-2">{refMaps.categoryById.get(l.transaction.categoryId ?? -1)?.name ?? '-'}</td>
                <td className="num">{won(l.amount)}</td>
              </tr>
            ))}
            {lines.length === 0 && (
              <tr>
                <td colSpan={4} className="empty">
                  이 달 결제분이 없습니다
                </td>
              </tr>
            )}
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={3}>결제 금액 ({lines.length}건)</td>
              <td className="num">
                {won(confirmed)}
                {planned !== 0 && <div className="small muted">+예정 {won(planned)}</div>}
              </td>
            </tr>
          </tfoot>
        </table>
      )}
    </Panel>
  )
}
