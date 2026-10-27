import { useEffect, useState } from 'react'

import { useGql, useRefData, type RefMaps } from '../api/hooks'
import { SUMMARY_QUERY, TRANSACTIONS_QUERY } from '../api/queries'
import type { MonthlySummary, TransactionPage, TxStatus } from '../api/types'
import { ErrorBox, Loading, Panel } from '../components/Common'
import LedgerTabs from '../components/LedgerTabs'
import MonthPicker from '../components/MonthPicker'
import TransactionGrid, { PageTotals, type GridColumn } from '../components/TransactionGrid'
import { won } from '../lib/format'
import { defaultDateFor, monthLabel, useMonthParam, useQueryParam } from '../lib/month'

const STATUS_FILTERS: { value: string; label: string; statuses: TxStatus[] }[] = [
  { value: 'active', label: '확정+예정', statuses: ['CONFIRMED', 'PLANNED'] },
  { value: 'CONFIRMED', label: '확정', statuses: ['CONFIRMED'] },
  { value: 'PLANNED', label: '예정', statuses: ['PLANNED'] },
  { value: 'SKIPPED', label: '건너뜀', statuses: ['SKIPPED'] },
]

export default function Transactions() {
  const { ref, error: refError } = useRefData()
  const [month, setMonth] = useMonthParam()
  const [ledgerParam, setLedgerParam] = useQueryParam('ledger', 'default')
  const [category, setCategory] = useQueryParam('category', '')
  const [pm, setPm] = useQueryParam('pm', '')
  const [status, setStatus] = useQueryParam('status', 'active')
  const [member, setMember] = useQueryParam('member', '')
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')

  useEffect(() => {
    const t = window.setTimeout(() => setDebouncedSearch(search.trim()), 300)
    return () => window.clearTimeout(t)
  }, [search])

  const ledgerId = ledgerParam === 'default' ? ref?.defaultLedger?.id ?? null : ledgerParam === 'all' ? null : Number(ledgerParam)
  const statuses = STATUS_FILTERS.find((s) => s.value === status)?.statuses ?? ['CONFIRMED', 'PLANNED']
  const filter = {
    month,
    ledgerId,
    categoryId: category && category !== 'none' ? Number(category) : null,
    uncategorized: category === 'none' ? true : null,
    paymentMethodId: pm ? Number(pm) : null,
    statuses,
    member: member || null,
    search: debouncedSearch || null,
  }
  const tx = useGql<{ transactions: TransactionPage }>(['transactions', filter], TRANSACTIONS_QUERY, { filter, sort: 'DATE_ASC', limit: 3000 }, { enabled: !!ref })
  const summary = useGql<{ monthlySummary: MonthlySummary }>(
    ['summary', month, ledgerId, '6', 'sheet'],
    SUMMARY_QUERY,
    { month, ledgerId, averageMonths: 6, historyMonths: 1 },
    { enabled: !!ref },
  )

  if (refError) return <ErrorBox error={refError} />
  if (!ref) return <Loading />

  const columns: GridColumn[] = ['date', 'amount', 'category', 'paymentMethod', 'description', 'member', 'memo']
  if (ledgerId == null) columns.push('ledger')
  const page = tx.data?.transactions
  const filtered = !!(category || pm || member || debouncedSearch || status !== 'active')

  return (
    <div className="stack">
      <div className="page-head">
        <h1>가계부</h1>
        <div className="filters">
          <MonthPicker month={month} onChange={setMonth} />
          <LedgerTabs
            ledgers={ref.ledgers}
            value={ledgerId == null ? '' : String(ledgerId)}
            onChange={(v) => {
              setLedgerParam(v === '' ? 'all' : v)
              setCategory('')
            }}
          />
        </div>
      </div>
      <div className="filters">
        <select className="input" value={category} onChange={(e) => setCategory(e.target.value)} aria-label="항목 필터">
          <option value="">모든 항목</option>
          <option value="none">미분류</option>
          {ref.categoriesOf(ledgerId).map((c) => (
            <option key={c.id} value={c.id}>
              {ledgerId == null ? `${ref.ledgerById.get(c.ledgerId)?.name} · ` : ''}
              {c.name}
            </option>
          ))}
        </select>
        <select className="input" value={pm} onChange={(e) => setPm(e.target.value)} aria-label="결제수단 필터">
          <option value="">모든 결제수단</option>
          {ref.paymentMethods.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
              {p.archived ? ' (보관)' : ''}
            </option>
          ))}
        </select>
        <select className="input" value={member} onChange={(e) => setMember(e.target.value)} aria-label="구성원 필터">
          <option value="">모든 구성원</option>
          {ref.members.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
        <div className="segmented" role="group" aria-label="상태">
          {STATUS_FILTERS.map((f) => (
            <button key={f.value} className={status === f.value ? 'on' : ''} onClick={() => setStatus(f.value)}>
              {f.label}
            </button>
          ))}
        </div>
        <input className="input" placeholder="내용·메모 검색" value={search} onChange={(e) => setSearch(e.target.value)} style={{ width: 180 }} />
        {filtered && (
          <button
            className="btn sm ghost"
            onClick={() => {
              setCategory('')
              setPm('')
              setMember('')
              setStatus('active')
              setSearch('')
            }}
          >
            필터 지우기
          </button>
        )}
      </div>

      <div className="sheet-layout">
        <section className={`panel ${tx.isFetching ? 'refetching' : ''}`} style={{ overflow: 'hidden' }}>
          {tx.error ? (
            <div style={{ padding: 16 }}>
              <ErrorBox error={tx.error} />
            </div>
          ) : !page ? (
            <Loading />
          ) : (
            <TransactionGrid
              rows={page.items}
              refMaps={ref}
              columns={columns}
              newRow={filtered ? undefined : { date: defaultDateFor(month), ledgerId }}
              emptyText={filtered ? '조건에 맞는 거래가 없습니다' : `${monthLabel(month)} 거래가 없습니다. 아래 줄에 바로 입력하세요.`}
              footer={
                <PageTotals page={page} />
              }
            />
          )}
        </section>
        <SheetSummary s={summary.data?.monthlySummary} refMaps={ref} onCategory={setCategory} onPm={setPm} />
      </div>
    </div>
  )
}

/** 스프레드시트 오른쪽 요약 블록과 같은 정보 */
function SheetSummary({
  s,
  refMaps,
  onCategory,
  onPm,
}: {
  s?: MonthlySummary
  refMaps: RefMaps
  onCategory: (v: string) => void
  onPm: (v: string) => void
}) {
  if (!s) return <Panel title="요약"><Loading /></Panel>
  return (
    <div className="stack" style={{ position: 'sticky', top: 12 }}>
      <Panel title="이번 달 요약" flush>
        <table className="table">
          <tbody>
            <tr>
              <td>총 사용 금액</td>
              <td className="num" style={{ fontWeight: 650 }}>
                {won(s.spending)}
              </td>
            </tr>
            {s.plannedSpending !== 0 && (
              <tr>
                <td className="text-2">+ 남은 예정</td>
                <td className="num text-2">{won(s.plannedSpending)}</td>
              </tr>
            )}
            {s.excludedSpending !== 0 && (
              <tr>
                <td className="text-2">합계 제외 (경조사·여행 등)</td>
                <td className="num text-2">{won(s.excludedSpending)}</td>
              </tr>
            )}
            {s.plan && (
              <>
                <tr>
                  <td className="text-2">계획 수입</td>
                  <td className="num text-2">{won(s.plan.income)}</td>
                </tr>
                {s.plan.items
                  .filter((i) => i.kind === 'ALLOCATION')
                  .map((i) => (
                    <tr key={i.id}>
                      <td className="text-2">− {i.name}</td>
                      <td className="num text-2">{won(i.amount)}</td>
                    </tr>
                  ))}
                <tr>
                  <td style={{ fontWeight: 650 }}>월잔액</td>
                  <td className={`num ${s.balance != null && s.balance < 0 ? 'bad' : ''}`} style={{ fontWeight: 650 }}>
                    {won(s.balance)}
                  </td>
                </tr>
              </>
            )}
            {s.members.map((m) => (
              <tr key={m.member}>
                <td className="text-2">
                  {m.member} 지출{m.allowance != null ? ` / 용돈 ${won(m.allowance)}` : ''}
                </td>
                <td className="num text-2">
                  {won(m.spending)}
                  {m.remaining != null && <div className="small">잔액 {won(m.remaining)}</div>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Panel>
      <Panel title="소비 총액 (항목별)" flush>
        <table className="table">
          <tbody>
            {s.categories
              .filter((c) => c.confirmed !== 0 || c.planned !== 0)
              .map((c) => (
                <tr key={`${c.categoryId}`} className="clickable" onClick={() => onCategory(c.categoryId == null ? 'none' : String(c.categoryId))}>
                  <td>
                    {s.ledgerId == null && c.ledgerId != null && <span className="muted">{refMaps.ledgerById.get(c.ledgerId)?.name} · </span>}
                    {c.name}
                    {c.kind === 'INCOME' && <span className="badge good" style={{ marginLeft: 6 }}>수입</span>}
                  </td>
                  <td className="num">
                    {won(c.confirmed)}
                    {c.planned !== 0 && <div className="small muted">+예정 {won(c.planned)}</div>}
                  </td>
                </tr>
              ))}
          </tbody>
        </table>
      </Panel>
      <Panel title="결제수단별" flush>
        <table className="table">
          <tbody>
            {s.paymentMethods.map((p) => (
              <tr key={`${p.paymentMethodId}`} className="clickable" onClick={() => p.paymentMethodId && onPm(String(p.paymentMethodId))}>
                <td>{p.paymentMethodId ? refMaps.paymentMethodById.get(p.paymentMethodId)?.name ?? p.name : p.name}</td>
                <td className="num">
                  {won(p.confirmed)}
                  {p.planned !== 0 && <div className="small muted">+예정 {won(p.planned)}</div>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Panel>
    </div>
  )
}
