import { useEffect, useState, type FormEvent } from 'react'

import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import { MOVE_TXS, SPLIT_TX, SUMMARY_QUERY, TRANSACTIONS_QUERY, UNSPLIT_TX } from '../api/queries'
import type { MonthlySummary, Transaction, TransactionPage, TxStatus } from '../api/types'
import { ErrorBox, Loading, Panel } from '../components/Common'
import BalanceRows from '../components/BalanceRows'
import DeleteSelectedButton from '../components/DeleteTransactions'
import LedgerTabs from '../components/LedgerTabs'
import Modal from '../components/Modal'
import MonthPicker from '../components/MonthPicker'
import { useToast } from '../components/Toast'
import TransactionGrid, { PageTotals, type GridColumn } from '../components/TransactionGrid'
import { parseAmount, shortDate, won } from '../lib/format'
import { defaultDateFor, monthLabel, useMonthParam, useQueryParam } from '../lib/month'
import { useTxSelection } from '../lib/selection'
import { isSplit, splitAmountError, splitOriginLedgerId, splitTotal } from '../lib/split'

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
  // 선택 모드: 체크한 거래를 지우거나 다른 장부로 옮긴다
  const [moveTarget, setMoveTarget] = useState('')
  // 분할이동 창에서 나눌 거래
  const [splitting, setSplitting] = useState<Transaction | null>(null)
  const { run, pending } = useMutate()
  const toast = useToast()

  useEffect(() => {
    const t = window.setTimeout(() => setDebouncedSearch(search.trim()), 300)
    return () => window.clearTimeout(t)
  }, [search])

  const ledgerId = ledgerParam === 'default' ? ref?.defaultLedger?.id ?? null : ledgerParam === 'all' ? null : Number(ledgerParam)
  const statuses = STATUS_FILTERS.find((s) => s.value === status)?.statuses ?? ['CONFIRMED', 'PLANNED']
  const filter = {
    month,
    ledgerId,
    categoryId: category && category !== 'none' && category !== 'transfer' ? Number(category) : null,
    uncategorized: category === 'none' ? true : null,
    transfers: category === 'transfer' ? true : null,
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
  const sel = useTxSelection(tx.data?.transactions.items)
  const { chosen, selecting, stop: stopSelecting } = sel

  if (refError) return <ErrorBox error={refError} />
  if (!ref) return <Loading />

  const columns: GridColumn[] = ['date', 'amount', 'category', 'paymentMethod', 'description', 'member', 'memo']
  if (ledgerId == null) columns.push('ledger')
  const page = tx.data?.transactions
  const filtered = !!(category || pm || member || debouncedSearch || status !== 'active')

  // 지금 보고 있는 장부가 아닌 장부로만 옮긴다 (전체 보기면 모든 장부)
  const targets = ref.ledgers.filter((l) => l.id !== ledgerId)
  const target = targets.find((l) => String(l.id) === moveTarget) ?? targets[0]
  const moveSelected = async () => {
    if (!target) return
    const ids = chosen.filter((t) => t.ledgerId !== target.id).map((t) => t.id)
    if (ids.length === 0) {
      toast(chosen.length ? `선택한 거래는 이미 '${target.name}' 장부에 있습니다` : '옮길 거래를 체크하세요', 'error')
      return
    }
    const ok = window.confirm(
      `선택한 ${ids.length}건을 '${target.name}' 장부로 옮길까요?\n'${target.name}'에 같은 이름의 항목이 없는 거래는 미분류가 됩니다.`,
    )
    if (!ok) return
    const res = await run<{ moveTransactions: { moved: number; uncategorized: number } }>(MOVE_TXS, { ids, ledgerId: target.id })
    if (!res) return
    const { moved, uncategorized } = res.moveTransactions
    toast(`${moved}건을 '${target.name}' 장부로 옮겼습니다${uncategorized ? ` (${uncategorized}건은 미분류)` : ''}`)
    stopSelecting()
  }

  // 분할한 결제는 통째로 옮기지 않는다. 한 건만 골랐을 때 나누거나(분할이동) 되돌린다(분할되돌리기).
  const single = chosen.length === 1 ? chosen[0] : undefined
  const moveBlocked = chosen.some(isSplit) ? '분할된 거래는 옮길 수 없습니다. 분할을 되돌린 뒤 옮기세요' : undefined
  const splitBlocked =
    ref.ledgers.length < 2
      ? '나눌 다른 장부가 없습니다'
      : !single
        ? '한 건만 골라야 나눌 수 있습니다'
        : single.status !== 'CONFIRMED'
          ? '확정된 거래만 나눌 수 있습니다'
          : single.transferLedgerId != null
            ? '장부 이체는 나눌 수 없습니다'
            : undefined
  const unsplitSelected = async () => {
    if (!single || !isSplit(single)) return
    const origin = ref.ledgerById.get(splitOriginLedgerId(single))?.name
    const total = won(splitTotal(single))
    if (!window.confirm(`분할을 되돌려 '${origin}' 장부의 결제 한 건(${total}원)으로 합칠까요?`)) return
    const res = await run(UNSPLIT_TX, { id: single.id })
    if (!res) return
    toast(`'${origin}' 장부의 ${total}원 한 건으로 합쳤습니다`)
    stopSelecting()
  }

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
          <option value="transfer">장부 이체</option>
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
        <button className={`btn sm ${selecting ? 'ghost' : ''}`} aria-pressed={selecting} onClick={selecting ? stopSelecting : sel.start}>
          {selecting ? '선택 취소' : '선택'}
        </button>
        {selecting && <span className="small text-2">{chosen.length}건 선택</span>}
        {selecting && target && (
          <>
            <select className="input" value={target.id} onChange={(e) => setMoveTarget(e.target.value)} aria-label="옮길 장부">
              {targets.map((l) => (
                <option key={l.id} value={l.id}>
                  → {l.name}
                </option>
              ))}
            </select>
            <button className="btn sm primary" disabled={pending || chosen.length === 0 || !!moveBlocked} title={moveBlocked} onClick={moveSelected}>
              이동
            </button>
            {single && isSplit(single) ? (
              <button className="btn sm" disabled={pending} onClick={unsplitSelected} title="나눈 금액을 원래 거래 한 건으로 합칩니다">
                분할되돌리기
              </button>
            ) : (
              <button
                className="btn sm"
                disabled={pending || !!splitBlocked}
                title={splitBlocked ?? '결제 금액 일부를 다른 장부로 나눕니다'}
                onClick={() => single && setSplitting(single)}
              >
                분할이동
              </button>
            )}
          </>
        )}
        {selecting && <DeleteSelectedButton chosen={chosen} refMaps={ref} onDone={stopSelecting} />}
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
              viewLedgerId={ledgerId}
              newRow={filtered ? undefined : { date: defaultDateFor(month), ledgerId }}
              selection={sel.grid}
              emptyText={filtered ? '조건에 맞는 거래가 없습니다' : `${monthLabel(month)} 거래가 없습니다. 아래 줄에 바로 입력하세요.`}
              footer={
                <PageTotals page={page} />
              }
            />
          )}
        </section>
        <SheetSummary s={summary.data?.monthlySummary} refMaps={ref} onCategory={setCategory} onPm={setPm} />
      </div>
      {splitting && (
        <SplitModal
          tx={splitting}
          refMaps={ref}
          defaultLedgerId={target?.id}
          onClose={() => setSplitting(null)}
          onDone={() => {
            setSplitting(null)
            stopSelecting()
          }}
        />
      )}
    </div>
  )
}

/** 결제 한 건의 일부 금액을 다른 장부로 나누기. 두 장부의 금액을 합치면 결제 금액이다. */
function SplitModal({
  tx,
  refMaps,
  defaultLedgerId,
  onClose,
  onDone,
}: {
  tx: Transaction
  refMaps: RefMaps
  defaultLedgerId?: number
  onClose: () => void
  onDone: () => void
}) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const ledgers = refMaps.ledgers.filter((l) => l.id !== tx.ledgerId)
  // 옮길 장부에서 원래 항목과 이름·종류가 같은 항목 (서버의 기본값과 같은 규칙)
  const sameCategory = (lid: number) => {
    const c = tx.categoryId ? refMaps.categoryById.get(tx.categoryId) : undefined
    const match = c && refMaps.categoriesOf(lid).find((t) => t.name === c.name && t.kind === c.kind)
    return match ? String(match.id) : ''
  }
  const [ledgerId, setLedgerId] = useState(() => (ledgers.find((l) => l.id === defaultLedgerId) ?? ledgers[0]).id)
  const [categoryId, setCategoryId] = useState(() => sameCategory(ledgerId))
  const [amountText, setAmountText] = useState('')
  const amount = parseAmount(amountText)
  const error = amountText.trim() ? splitAmountError(tx.amount, amount) : null
  const here = refMaps.ledgerById.get(tx.ledgerId)?.name
  const there = refMaps.ledgerById.get(ledgerId)?.name

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    const err = splitAmountError(tx.amount, amount)
    if (err || amount == null) {
      toast(err ?? '금액을 입력하세요', 'error')
      return
    }
    const res = await run(SPLIT_TX, { id: tx.id, ledgerId, amount, categoryId: categoryId ? Number(categoryId) : null })
    if (!res) return
    toast(`${won(amount)}원을 '${there}' 장부로 나눴습니다`)
    onDone()
  }

  return (
    <Modal
      title="분할이동"
      onClose={onClose}
      width={440}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            취소
          </button>
          <button className="btn primary" form="split-form" disabled={pending || amount == null || !!error}>
            나누기
          </button>
        </>
      }
    >
      <form id="split-form" className="stack" onSubmit={submit}>
        <div className="small text-2">
          {shortDate(tx.date)} · {tx.description}
          {tx.paymentMethodId ? ` · ${refMaps.paymentMethodById.get(tx.paymentMethodId)?.name}` : ''} · 결제{' '}
          <b className="num">{won(tx.amount)}원</b>
        </div>
        <label className="field">
          옮길 장부
          <select
            className="input"
            value={ledgerId}
            onChange={(e) => {
              const next = Number(e.target.value)
              setLedgerId(next)
              setCategoryId(sameCategory(next))
            }}
          >
            {ledgers.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          옮길 금액
          <input
            className="input num"
            inputMode="numeric"
            autoFocus
            placeholder={tx.amount < 0 ? '예: -5000' : '예: 5000'}
            value={amountText}
            onChange={(e) => setAmountText(e.target.value)}
          />
        </label>
        {error && <div className="small bad">{error}</div>}
        <label className="field">
          {there} 항목
          <select className="input" value={categoryId} onChange={(e) => setCategoryId(e.target.value)}>
            <option value="">미분류</option>
            {refMaps.categoriesOf(ledgerId).map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </label>
        {amount != null && !error && (
          <div className="small">
            {here} <b className="num">{won(tx.amount - amount)}원</b> + {there} <b className="num">{won(amount)}원</b> = 결제{' '}
            <span className="num">{won(tx.amount)}원</span>
          </div>
        )}
      </form>
    </Modal>
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
            {s.plan && <BalanceRows s={s} refMaps={refMaps} />}
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
