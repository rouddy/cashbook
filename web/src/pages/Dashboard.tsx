import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import { CARDS_MONTHLY_QUERY, CONFIRM_TX, FUNDS_QUERY, SKIP_TX, SUMMARY_QUERY, TRANSACTIONS_QUERY } from '../api/queries'
import type { AvailableFunds, CardMonthReport, MonthlySummary, Transaction, TransactionPage } from '../api/types'
import BalanceRows from '../components/BalanceRows'
import ColumnChart from '../components/charts/ColumnChart'
import LineChart from '../components/charts/LineChart'
import { Delta, ErrorBox, Loading, Meter, Panel, PerformanceMeter, PlannedSwatch, StatTile } from '../components/Common'
import LedgerTabs from '../components/LedgerTabs'
import MonthPicker from '../components/MonthPicker'
import { benefitTotalsText, plannedPerformance } from '../lib/benefits'
import { compactWon, longDate, percent, shortDate, signedWon, won } from '../lib/format'
import { addDays, currentMonth, localToday, monthLabel, shortMonthLabel, useMonthParam, useQueryParam } from '../lib/month'
import { transferDirection, transferLabel } from '../lib/transfer'

const AVG_OPTIONS = ['3', '6', '12']

export default function Dashboard() {
  const { ref, error: refError } = useRefData()
  const [month, setMonth] = useMonthParam()
  const [ledgerParam, setLedgerParam] = useQueryParam('ledger', 'default')
  const [avg, setAvg] = useQueryParam('avg', '6')

  const ledgerId = ledgerParam === 'default' ? ref?.defaultLedger?.id ?? null : ledgerParam === 'all' ? null : Number(ledgerParam)
  const ledgerValue = ledgerId == null ? '' : String(ledgerId)

  const summary = useGql<{ monthlySummary: MonthlySummary }>(
    ['summary', month, ledgerId, avg],
    SUMMARY_QUERY,
    { month, ledgerId, averageMonths: Number(avg), historyMonths: 12 },
    { enabled: !!ref },
  )
  const funds = useGql<{ availableFunds: AvailableFunds }>(['funds'], FUNDS_QUERY)

  if (refError) return <ErrorBox error={refError} />
  if (!ref) return <Loading />

  const s = summary.data?.monthlySummary

  return (
    <div className="stack">
      <div className="page-head">
        <h1>대시보드</h1>
        <div className="filters">
          <MonthPicker month={month} onChange={setMonth} />
          <LedgerTabs ledgers={ref.ledgers} value={ledgerValue} onChange={(v) => setLedgerParam(v === '' ? 'all' : v)} />
          <div className="segmented" role="group" aria-label="평균 기간">
            {AVG_OPTIONS.map((o) => (
              <button key={o} className={avg === o ? 'on' : ''} onClick={() => setAvg(o)}>
                {o}개월 평균
              </button>
            ))}
          </div>
        </div>
      </div>

      {summary.error && <ErrorBox error={summary.error} />}
      {!s ? (
        <Loading />
      ) : (
        <div className={`stack ${summary.isFetching ? 'refetching' : ''}`}>
          <Kpis s={s} />
          <div className="grid-2">
            <DailyPanel s={s} />
            <FundsPanel funds={funds.data?.availableFunds} error={funds.error} refMaps={ref} />
          </div>
          <div className="grid-2">
            <CategoryPanel s={s} ledgerId={ledgerId} refMaps={ref} />
            <PlannedPanel refMaps={ref} ledgerId={ledgerId} />
          </div>
          <HistoryPanel s={s} onSelect={setMonth} />
          <CardPlanPanel month={month} refMaps={ref} />
          <div className="grid-2">
            <MethodPanel s={s} />
            <MemberPanel s={s} refMaps={ref} />
          </div>
        </div>
      )}
    </div>
  )
}

function Kpis({ s }: { s: MonthlySummary }) {
  const projected = s.spending + s.plannedSpending
  const diff = s.spending - s.averageSpending
  return (
    <div className="kpis">
      <StatTile
        label={`${monthLabel(s.month)} 소비`}
        value={`${won(s.spending)}원`}
        delta={
          s.averageMonthsUsed > 0 ? (
            <>
              <Delta value={Math.round(diff)} /> <span className="text-2">· 평균 {won(s.averageSpending)}원</span>
            </>
          ) : (
            <span className="text-2">비교할 지난 데이터 없음</span>
          )
        }
      />
      <StatTile
        label="예정 포함 예상 소비"
        value={`${won(projected)}원`}
        delta={<span className="text-2">남은 예정 {won(s.plannedSpending)}원</span>}
      />
      <StatTile
        label="예산 대비"
        value={s.budgetTotal ? percent(projected / s.budgetTotal) : '예산 없음'}
        delta={s.budgetTotal ? <span className="text-2">예산 {won(s.budgetTotal)}원 · 예정 포함</span> : <span className="text-2">지출 계획에서 카테고리 예산을 정하세요</span>}
      >
        {s.budgetTotal ? (
          <div style={{ marginTop: 8 }}>
            <Meter value={projected} max={s.budgetTotal} ticks={[s.spending]} label="예산 대비 예정 포함 소비" />
          </div>
        ) : null}
      </StatTile>
      {s.balance != null ? (
        <StatTile
          label="월잔액"
          value={<span className={s.balance < 0 ? 'bad' : ''}>{won(s.balance)}원</span>}
          delta={<span className="text-2">예정(월급·이체 등) 반영 시 {won(s.projectedBalance ?? s.balance)}원</span>}
        />
      ) : (
        <StatTile label="수입" value={`${won(s.income)}원`} delta={<span className="text-2">수입 카테고리 합계</span>} />
      )}
    </div>
  )
}

function DailyPanel({ s }: { s: MonthlySummary }) {
  const [asTable, setAsTable] = useState(false)
  const today = localToday()
  const isCurrent = s.month === currentMonth()
  const isFuture = s.month > currentMonth()
  const todayIdx = isCurrent ? Number(today.slice(8, 10)) - 1 : isFuture ? -1 : s.daily.length - 1
  const labels = s.daily.map((d) => `${d.day}일`)
  const confirmed = s.daily.map((d, i) => (i <= todayIdx ? d.cumulative : null))
  const hasPlanned = s.plannedSpending !== 0
  const series = [
    { key: 'avg', name: `${s.averageMonthsUsed}개월 평균`, values: s.daily.map((d) => d.averageCumulative), color: 'var(--series-context)', endLabel: true },
    ...(hasPlanned
      ? [{ key: 'proj', name: '예정 포함', values: s.daily.map((d) => d.projectedCumulative), color: 'var(--accent)', opacity: 0.45, endLabel: true }]
      : []),
    { key: 'cur', name: '이번 달 소비', values: confirmed, color: 'var(--accent)', area: true, endLabel: true },
  ]
  const n = s.daily.length
  return (
    <Panel
      title="일별 누적 소비"
      sub={`소비 합계 기준 (수입·장부 이체 제외) · ${monthLabel(s.month)}`}
      actions={
        <button className="btn sm ghost" onClick={() => setAsTable((v) => !v)}>
          {asTable ? '차트로 보기' : '표로 보기'}
        </button>
      }
    >
      {asTable ? (
        <div className="table-wrap" style={{ maxHeight: 300, overflowY: 'auto' }}>
          <table className="table">
            <thead>
              <tr>
                <th>날짜</th>
                <th className="num">당일</th>
                <th className="num">누적</th>
                <th className="num">예정 포함</th>
                <th className="num">평균 누적</th>
              </tr>
            </thead>
            <tbody>
              {s.daily.map((d) => (
                <tr key={d.day}>
                  <td>{shortDate(d.date)}</td>
                  <td className="num">{won(d.spending)}</td>
                  <td className="num">{won(d.cumulative)}</td>
                  <td className="num">{won(d.projectedCumulative)}</td>
                  <td className="num">{won(d.averageCumulative)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <LineChart
          labels={labels}
          series={series}
          height={250}
          xTicks={[0, 9, 19, n - 1].filter((i) => i < n)}
          titleFor={(i) => shortDate(s.daily[i].date)}
          ariaLabel={`${monthLabel(s.month)} 일별 누적 소비와 평균 비교`}
        />
      )}
    </Panel>
  )
}

function FundsPanel({ funds, error, refMaps }: { funds?: AvailableFunds; error: unknown; refMaps: RefMaps }) {
  if (error) return <ErrorBox error={error} />
  if (!funds) return <Panel title="여유자금"><Loading /></Panel>
  const noAccounts = funds.accountsTotal === 0 && !funds.balanceDate
  const upcoming = funds.cards
    .filter((c) => c.nextPayment && (c.nextPayment.confirmedAmount !== 0 || c.nextPayment.plannedAmount !== 0))
    .sort((a, b) => (a.nextPayment!.paymentDate < b.nextPayment!.paymentDate ? -1 : 1))
  return (
    <Panel
      title="여유자금"
      sub={funds.balanceDate ? `잔고 기준일 ${longDate(funds.balanceDate)}` : '은행 잔고를 입력하면 계산됩니다'}
      actions={<Link to="/accounts" className="btn sm">잔고 입력</Link>}
    >
      <div className="hero" style={{ marginBottom: 12 }}>
        <span className={funds.available < 0 ? 'bad' : ''}>{won(funds.available)}</span>
        <span style={{ fontSize: 18, fontWeight: 600 }}>원</span>
      </div>
      <div className="breakdown">
        <span className="text-2">은행 잔고 합계</span>
        <span className="num">{won(funds.accountsTotal)}</span>
        <span className="text-2">− 카드 결제 예정 (확정 이용분)</span>
        <span className="num">{won(funds.cardDue)}</span>
        {funds.unreflectedOutflow !== 0 && (
          <>
            <span className="text-2">− 잔고 반영 전 계좌 출금</span>
            <span className="num">{won(funds.unreflectedOutflow)}</span>
          </>
        )}
        <span className="total">= 현재 여유자금</span>
        <span className="num total">{won(funds.available)}</span>
        <span className="text-2">− 이번 달까지 예정 지출 (카드 {won(funds.cardDuePlanned)} 포함)</span>
        <span className="num">{won(funds.plannedOutflow + funds.cardDuePlanned)}</span>
        {funds.plannedInflow !== 0 && (
          <>
            <span className="text-2">+ 예정 수입</span>
            <span className="num">{won(funds.plannedInflow)}</span>
          </>
        )}
        <span className="total">= 예정 반영 후</span>
        <span className={`num total ${funds.availableAfterPlanned < 0 ? 'bad' : ''}`}>{won(funds.availableAfterPlanned)}</span>
      </div>
      {noAccounts && (
        <div className="notice info" style={{ marginTop: 12 }}>
          <span className="notice-icon">i</span>
          <span>
            <Link to="/accounts">은행 잔고</Link>를 입력하면 카드 결제 예정 금액을 빼고 쓸 수 있는 돈을 보여줍니다.
          </span>
        </div>
      )}
      {funds.unconfiguredCardIds.length > 0 && (
        <div className="notice" style={{ marginTop: 12 }}>
          <span className="notice-icon">!</span>
          <span>
            정산 일정이 없는 카드가 있어 카드대금에서 빠졌습니다:{' '}
            {funds.unconfiguredCardIds.map((id, i) => (
              <span key={id}>
                {i > 0 && ', '}
                <Link to={`/cards/${id}`}>{refMaps.paymentMethodById.get(id)?.name ?? id}</Link>
              </span>
            ))}
          </span>
        </div>
      )}
      {upcoming.length > 0 && (
        <>
          <h3 style={{ marginTop: 16, marginBottom: 6 }}>다가오는 카드 결제</h3>
          <table className="table">
            <tbody>
              {upcoming.map((c) => {
                const p = c.nextPayment!
                return (
                  <tr key={c.paymentMethodId}>
                    <td>
                      <Link to={`/cards/${c.paymentMethodId}`}>{refMaps.paymentMethodById.get(c.paymentMethodId)?.name}</Link>
                      <div className="small muted">
                        {shortDate(p.periodStart, false)}~{shortDate(p.periodEnd, false)} 이용{p.closed ? '' : ' (진행 중)'}
                        {!c.counted && ' · 여유자금 제외 계좌에서 출금'}
                      </div>
                    </td>
                    <td className="nowrap">{shortDate(p.paymentDate)}</td>
                    <td className="num">
                      {won(p.confirmedAmount)}
                      {p.plannedAmount !== 0 && <div className="small muted">+ 예정 {won(p.plannedAmount)}</div>}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </>
      )}
    </Panel>
  )
}

function CategoryPanel({ s, ledgerId, refMaps }: { s: MonthlySummary; ledgerId: number | null; refMaps: RefMaps }) {
  const navigate = useNavigate()
  const rows = s.categories.filter((c) => c.kind === 'EXPENSE')
  const max = Math.max(1, ...rows.map((c) => Math.max(c.confirmed + c.planned, c.average, c.budget ?? 0)))
  const goTo = (categoryId: number | null) => {
    const q = new URLSearchParams({
      month: s.month,
      ledger: ledgerId == null ? 'all' : String(ledgerId),
      category: categoryId == null ? 'none' : String(categoryId),
    })
    navigate(`/transactions?${q}`)
  }
  return (
    <Panel title="카테고리별 이번 달 vs 평균" sub={`막대 = 이번 달(옅은 부분은 예정), 세로선 = ${s.averageMonthsUsed}개월 평균`} flush>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>카테고리</th>
              <th className="hide-sm" style={{ width: '36%' }}>
                <span className="sr-only">비교 막대</span>
              </th>
              <th className="num">이번 달</th>
              <th className="num">평균</th>
              <th className="num">예산</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((c) => {
              const total = c.confirmed + c.planned
              const over = c.budget != null && total > c.budget
              return (
                <tr key={`${c.categoryId}`} className="clickable" onClick={() => goTo(c.categoryId)}>
                  <td className="nowrap">
                    {ledgerId == null && c.ledgerId != null && <span className="muted">{refMaps.ledgerById.get(c.ledgerId)?.name} · </span>}
                    {c.name}
                    {c.isFixed && <span className="badge" style={{ marginLeft: 6 }}>고정</span>}
                  </td>
                  <td className="hide-sm">
                    <CompareBar confirmed={c.confirmed} planned={c.planned} average={c.average} max={max} />
                  </td>
                  <td className="num">
                    {won(c.confirmed)}
                    {c.planned !== 0 && <div className="small muted">+예정 {won(c.planned)}</div>}
                  </td>
                  <td className="num text-2">{won(c.average)}</td>
                  <td className={`num ${over ? 'bad' : 'text-2'}`}>
                    {c.budget != null ? won(c.budget) : '-'}
                    {over && <div className="small">▲ {won(total - c.budget!)} 초과</div>}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
    </Panel>
  )
}

function CompareBar({ confirmed, planned, average, max }: { confirmed: number; planned: number; average: number; max: number }) {
  const w = (v: number) => `${(Math.max(0, v) / max) * 100}%`
  return (
    <div style={{ position: 'relative', height: 14 }} title={`이번 달 ${won(confirmed)} / 예정 ${won(planned)} / 평균 ${won(average)}`}>
      <div style={{ position: 'absolute', inset: '3px 0', display: 'flex', gap: 2 }}>
        <span style={{ width: w(confirmed), background: 'var(--accent)', borderRadius: confirmed > 0 && planned <= 0 ? '0 4px 4px 0' : 0 }} />
        {planned > 0 && <span style={{ width: w(planned), background: 'var(--accent)', opacity: 0.35, borderRadius: '0 4px 4px 0' }} />}
      </div>
      {average > 0 && (
        <span style={{ position: 'absolute', top: 0, bottom: 0, left: `calc(${w(average)} - 1px)`, width: 2, background: 'var(--text-2)' }} />
      )}
    </div>
  )
}

function PlannedPanel({ refMaps, ledgerId }: { refMaps: RefMaps; ledgerId: number | null }) {
  const today = localToday()
  const from = addDays(today, -92)
  const to = addDays(today, 31)
  const q = useGql<{ transactions: TransactionPage }>(['planned', from, to, ledgerId], TRANSACTIONS_QUERY, {
    filter: { from, to, statuses: ['PLANNED'], ledgerId },
    sort: 'DATE_ASC',
    limit: 300,
  })
  const { run, pending } = useMutate()
  const items = q.data?.transactions.items ?? []
  const overdue = items.filter((t) => t.date < today)
  const upcoming = items.filter((t) => t.date >= today)
  // 합계는 지출만 (수입과 장부 이체는 따로 표시)
  const isSpending = (t: Transaction) => !t.isIncome && t.transferLedgerId == null
  const spendingOf = (list: Transaction[]) => list.filter(isSpending).reduce((a, t) => a + t.amount, 0)
  const row = (t: Transaction, isOverdue: boolean) => {
    const plus = t.isIncome || transferDirection(t, ledgerId) === 'in'
    return (
      <tr key={t.id}>
        <td className="nowrap">{shortDate(t.date)}</td>
        <td>
          {t.description}
          <div className="small muted">
            {[
              transferLabel(t, refMaps, ledgerId) ?? refMaps.categoryById.get(t.categoryId ?? -1)?.name,
              refMaps.paymentMethodById.get(t.paymentMethodId ?? -1)?.name,
            ]
              .filter(Boolean)
              .join(' · ')}
          </div>
        </td>
        <td className={`num ${plus ? 'good' : ''}`}>{plus ? `+${won(t.amount)}` : won(t.amount)}</td>
        <td className="nowrap" style={{ textAlign: 'right' }}>
          <button className="btn sm" disabled={pending} onClick={() => run(CONFIRM_TX, { id: t.id })} title="이 금액·날짜로 확정">
            확정
          </button>{' '}
          {isOverdue && (
            <button className="btn sm ghost" disabled={pending} onClick={() => run(SKIP_TX, { id: t.id })}>
              건너뛰기
            </button>
          )}
        </td>
      </tr>
    )
  }
  return (
    <Panel title="예정 거래" sub="반복 규칙(지출·월급·장부 이체)과 예정으로 입력한 거래 · 실제로 오가면 확정하세요" flush>
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorBox error={q.error} />
      ) : (
        <div className="table-wrap" style={{ maxHeight: 420, overflowY: 'auto' }}>
          {overdue.length > 0 && (
            <div className="notice" style={{ margin: '0 12px 8px' }}>
              <span className="notice-icon">!</span>
              <span>
                날짜가 지났는데 확정되지 않은 예정 항목 {overdue.length}건 (지출 {won(spendingOf(overdue))}원)
              </span>
            </div>
          )}
          <table className="table">
            <tbody>
              {overdue.map((t) => row(t, true))}
              {upcoming.length === 0 && overdue.length === 0 && (
                <tr>
                  <td className="empty">예정된 거래가 없습니다</td>
                </tr>
              )}
              {upcoming.length > 0 && (
                <tr>
                  <td colSpan={4} className="small text-2" style={{ background: 'var(--surface-2)' }}>
                    앞으로 31일 · 지출 {won(spendingOf(upcoming))}원
                  </td>
                </tr>
              )}
              {upcoming.map((t) => row(t, false))}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  )
}

function HistoryPanel({ s, onSelect }: { s: MonthlySummary; onSelect: (m: string) => void }) {
  const values = s.history.map((h) => h.spending)
  const highlight = s.history.findIndex((h) => h.month === s.month)
  const withData = s.history.filter((h) => h.spending !== 0)
  const average = withData.length ? withData.reduce((a, h) => a + h.spending, 0) / withData.length : 0
  return (
    <Panel title="월별 소비 추이" sub="최근 12개월 · 막대를 누르면 그 달로 이동">
      <ColumnChart
        labels={s.history.map((h) => shortMonthLabel(h.month))}
        values={values}
        highlight={highlight >= 0 ? highlight : undefined}
        reference={{ value: average, label: '12개월 평균' }}
        titleFor={(i) => monthLabel(s.history[i].month)}
        detailFor={(i) => {
          const h = s.history[i]
          const parts = [`고정 ${compactWon(h.fixedSpending)}`]
          if (h.plannedSpending) parts.push(`예정 ${compactWon(h.plannedSpending)}`)
          return parts.join(' · ')
        }}
        onSelect={(i) => onSelect(s.history[i].month)}
        ariaLabel="최근 12개월 월별 소비 막대 차트"
      />
    </Panel>
  )
}

function MethodPanel({ s }: { s: MonthlySummary }) {
  return (
    <Panel title="결제수단별 이번 달 사용액" flush>
      {s.paymentMethods.length === 0 ? (
        <div className="empty">거래가 없습니다</div>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>결제수단</th>
              <th className="num">확정</th>
              <th className="num">예정</th>
              <th className="num">건수</th>
            </tr>
          </thead>
          <tbody>
            {s.paymentMethods.map((p) => (
              <tr key={`${p.paymentMethodId}`}>
                <td>{p.paymentMethodId ? <Link to={`/cards/${p.paymentMethodId}`}>{p.name}</Link> : p.name}</td>
                <td className="num">{won(p.confirmed)}</td>
                <td className="num text-2">{p.planned ? won(p.planned) : '-'}</td>
                <td className="num text-2">{p.count}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  )
}

function MemberPanel({ s, refMaps }: { s: MonthlySummary; refMaps: RefMaps }) {
  return (
    <Panel title="구성원 · 지출 계획" flush>
      <table className="table">
        <tbody>
          {s.plan && <BalanceRows s={s} refMaps={refMaps} showSpending />}
          {s.members.map((m) => (
            <tr key={m.member}>
              <td>
                {m.member} 지출
                {m.allowance != null && <div className="small muted">용돈 {won(m.allowance)}원</div>}
              </td>
              <td className="num">
                {won(m.spending)}
                {m.remaining != null && (
                  <div className={`small ${m.remaining < 0 ? 'bad' : 'muted'}`}>잔액 {won(m.remaining)}</div>
                )}
              </td>
            </tr>
          ))}
          {!s.plan && s.members.length === 0 && (
            <tr>
              <td className="empty">장부를 선택하면 지출 계획 기준 월잔액을 보여줍니다</td>
            </tr>
          )}
        </tbody>
      </table>
    </Panel>
  )
}

/** 카드 소비 계획: 카드별 이번 달 실적이 목표(전월실적 기준)까지 얼마나 남았는지와 혜택 사용 */
function CardPlanPanel({ month, refMaps }: { month: string; refMaps: RefMaps }) {
  const q = useGql<{ cardsMonthly: CardMonthReport[] }>(['cardsMonthly', month], CARDS_MONTHLY_QUERY, { month })
  const rows = (q.data?.cardsMonthly ?? []).filter((r) => r.target || r.currentLimit != null || r.spending !== 0 || r.plannedSpending !== 0)
  return (
    <Panel title="카드 실적 현황" sub={`${monthLabel(month)} · 실적이 목표(전월실적 기준)에 닿아야 다음 달 혜택을 받습니다`} flush>
      {q.error ? (
        <ErrorBox error={q.error} />
      ) : !q.data ? (
        <Loading />
      ) : rows.length === 0 ? (
        <div className="empty">카드 사용 내역이 없습니다</div>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>카드</th>
                <th className="hide-sm" style={{ width: '28%' }}>
                  실적 / 목표
                </th>
                <th className="num">실적</th>
                <th className="num">목표까지</th>
                <th className="num">다음 달 한도</th>
                <th className="num">이번 달 혜택</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => {
                const pm = refMaps.paymentMethodById.get(r.paymentMethodId)
                return (
                  <tr key={r.paymentMethodId}>
                    <td className="nowrap">
                      <Link to={`/cards/${r.paymentMethodId}?month=${month}`}>{pm?.name}</Link>
                    </td>
                    <td className="hide-sm">
                      {r.target ? <PerformanceMeter report={r} label={`${pm?.name} 실적`} /> : <span className="muted small">목표 없음</span>}
                    </td>
                    <td className="num">
                      {won(r.performance)}
                      {r.target ? <span className="muted"> / {won(r.target)}</span> : null}
                      {plannedPerformance(r) !== 0 && (
                        <div className="small muted">
                          {plannedPerformance(r) > 0 && <PlannedSwatch />}
                          예정 {signedWon(plannedPerformance(r))} · 포함 {won(r.projectedPerformance)}
                        </div>
                      )}
                    </td>
                    <td className="num">
                      {r.target == null ? '-' : r.remainingToTarget ? <b>{won(r.remainingToTarget)}</b> : <span className="good">✓ 달성</span>}
                    </td>
                    <td className="num text-2">
                      {r.achievedTier ? won(r.achievedTier.benefitLimit) : r.nextTier ? '구간 미달' : '-'}
                      {r.nextTier && r.remainingToNextTier != null && (
                        <div className="small muted">
                          +{won(r.remainingToNextTier)} → {won(r.nextTier.benefitLimit)}
                        </div>
                      )}
                    </td>
                    <td className="num">
                      {benefitTotalsText(r)}
                      {r.currentLimit != null && (
                        <div className="small muted">
                          한도 {won(r.limitUsed)} / {won(r.currentLimit)}
                        </div>
                      )}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  )
}
