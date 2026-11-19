import { Fragment, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'

import { type RefMaps, useMutate } from '../api/hooks'
import { CONFIRM_TX, CREATE_TX, RESTORE_TX, SKIP_TX, UPDATE_TX } from '../api/queries'
import type { SplitPart, Suggestion, Transaction, TransactionPage, TxStatus } from '../api/types'
import { parseAmount, shortDate, SOURCE_LABEL, weekendClass, won } from '../lib/format'
import { localToday } from '../lib/month'
import { isSplit, splitTotal } from '../lib/split'
import BenefitChecks from './BenefitChecks'
import DescriptionInput from './DescriptionInput'
import { useToast } from './Toast'

export type GridColumn =
  | 'date'
  | 'amount'
  | 'category'
  | 'paymentMethod'
  | 'description'
  | 'member'
  | 'memo'
  | 'ledger'
  | 'benefits'
  | 'benefitAmount'

const HEADERS: Record<GridColumn, { label: string; width?: number; num?: boolean }> = {
  date: { label: '날짜', width: 84 },
  amount: { label: '금액', width: 96, num: true },
  category: { label: '항목', width: 88 },
  paymentMethod: { label: '결제수단', width: 108 },
  // 내용은 남는 폭을 쓴다
  description: { label: '내용' },
  member: { label: '구성원', width: 60 },
  memo: { label: '메모', width: 110 },
  ledger: { label: '장부', width: 64 },
  benefits: { label: '혜택', width: 120 },
  benefitAmount: { label: '혜택액', width: 72, num: true },
}

/** 편집 가능한 열 (혜택액은 계산값) */
const EDITABLE: GridColumn[] = ['date', 'amount', 'category', 'paymentMethod', 'description', 'member', 'memo', 'ledger', 'benefits']

interface Props {
  rows: Transaction[]
  refMaps: RefMaps
  columns: GridColumn[]
  /** 새 행 입력 (없으면 새 행을 보여주지 않는다) */
  newRow?: { date: string; ledgerId: number | null; paymentMethodId?: number | null }
  /** 거래별 계산된 혜택액 표시 (카드 화면, `txBenefitTexts`) */
  benefitAmounts?: Map<number, string>
  emptyText?: string
  footer?: ReactNode
  /** 선택 모드: 행 번호 대신 체크박스를 보이고 새 행 입력은 숨긴다 (삭제·장부 이동은 선택 모드에서만) */
  selection?: { selected: ReadonlySet<number>; onChange: (next: Set<number>) => void }
}

type Patch = Record<string, unknown>
type Move = 'next' | 'prev' | 'down' | 'stay'

export default function TransactionGrid({
  rows,
  refMaps,
  columns,
  newRow,
  benefitAmounts,
  emptyText = '거래가 없습니다',
  footer,
  selection,
}: Props) {
  const gridId = useId()
  const toast = useToast()
  const { run, pending } = useMutate()
  const [editing, setEditing] = useState<{ id: number; col: GridColumn } | null>(null)
  const [overrides, setOverrides] = useState<Record<number, Partial<Transaction>>>({})
  /** Shift+클릭 범위 선택의 시작 행 (거래 id) */
  const lastChecked = useRef<number | null>(null)
  const today = localToday()

  const view = rows.map((r) => (overrides[r.id] ? { ...r, ...overrides[r.id] } : r))
  const visibleIds = new Set(view.map((t) => t.id))

  /** 행 체크. Shift+클릭하면 직전에 누른 행부터 이 행까지 같은 상태로 바꾼다. */
  const checkRow = (index: number, on: boolean, shift: boolean) => {
    if (!selection) return
    const last = shift ? view.findIndex((t) => t.id === lastChecked.current) : -1
    const from = last >= 0 ? last : index
    const next = new Set(selection.selected)
    for (let i = Math.min(from, index); i <= Math.max(from, index); i++) {
      if (on) next.add(view[i].id)
      else next.delete(view[i].id)
    }
    lastChecked.current = view[index].id
    selection.onChange(next)
  }

  const focusCell = (r: number, c: number) => {
    const el = document.querySelector<HTMLElement>(`[data-cell="${CSS.escape(`${gridId}:${r}:${c}`)}"]`)
    el?.focus()
  }

  const save = async (tx: Transaction, input: Patch, optimistic: Partial<Transaction>) => {
    setOverrides((o) => ({ ...o, [tx.id]: { ...o[tx.id], ...optimistic } }))
    try {
      // 실패하면 토스트로 알리고, 아래에서 화면을 서버 값으로 되돌린다.
      await run(UPDATE_TX, { id: tx.id, input })
    } finally {
      setOverrides((o) => {
        const next = { ...o }
        delete next[tx.id]
        return next
      })
    }
  }

  const commit = (tx: Transaction, col: GridColumn, raw: string | number[] | null) => {
    switch (col) {
      case 'date': {
        const v = String(raw ?? '')
        if (!/^\d{4}-\d{2}-\d{2}$/.test(v) || v === tx.date) return
        return save(tx, { date: v }, { date: v })
      }
      case 'amount': {
        const v = parseAmount(String(raw ?? ''))
        if (v == null) {
          toast('금액은 숫자로 입력하세요 (환불은 -)', 'error')
          return
        }
        if (v === tx.amount) return
        return save(tx, { amount: v }, { amount: v })
      }
      case 'category': {
        const v = raw === '' || raw == null ? null : Number(raw)
        if (v === tx.categoryId) return
        return save(tx, { categoryId: v }, { categoryId: v })
      }
      case 'paymentMethod': {
        const v = raw === '' || raw == null ? null : Number(raw)
        if (v === tx.paymentMethodId) return
        return save(tx, { paymentMethodId: v }, { paymentMethodId: v })
      }
      case 'ledger': {
        const v = Number(raw)
        if (!v || v === tx.ledgerId) return
        return save(tx, { ledgerId: v }, { ledgerId: v })
      }
      case 'description': {
        const v = String(raw ?? '').trim()
        if (!v) {
          toast('내용을 비울 수 없습니다', 'error')
          return
        }
        if (v === tx.description) return
        return save(tx, { description: v }, { description: v })
      }
      case 'member':
      case 'memo': {
        const v = String(raw ?? '').trim()
        const current = (col === 'member' ? tx.member : tx.memo) ?? ''
        if (v === current) return
        return save(tx, { [col]: v === '' ? null : v }, { [col]: v === '' ? null : v })
      }
      case 'benefits': {
        const ids = (raw as number[]) ?? []
        return save(tx, { benefitIds: ids }, { benefitIds: ids })
      }
    }
  }

  const display = (tx: Transaction, col: GridColumn, prev?: Transaction): ReactNode => {
    switch (col) {
      case 'date':
        return (
          <span className={prev && prev.date === tx.date ? 'muted' : weekendClass(tx.date)}>
            {shortDate(tx.date)}
          </span>
        )
      case 'amount':
        return <span className={tx.amount < 0 || tx.isIncome ? 'good' : ''}>{tx.isIncome ? `+${won(tx.amount)}` : won(tx.amount)}</span>
      case 'category':
        return tx.categoryId ? refMaps.categoryById.get(tx.categoryId)?.name : <span className="placeholder muted">미분류</span>
      case 'paymentMethod':
        return tx.paymentMethodId ? refMaps.paymentMethodById.get(tx.paymentMethodId)?.name : <span className="muted">-</span>
      case 'description':
        return tx.description
      case 'member':
        return tx.member ?? ''
      case 'memo':
        return tx.memo ?? ''
      case 'ledger':
        return refMaps.ledgerById.get(tx.ledgerId)?.name
      case 'benefits': {
        const names = tx.benefitIds
          .map((id) => refMaps.paymentMethodById.get(tx.paymentMethodId ?? -1)?.benefits.find((b) => b.id === id)?.name)
          .filter(Boolean)
        return names.length ? names.join(', ') : <span className="muted">-</span>
      }
      case 'benefitAmount': {
        const v = benefitAmounts?.get(tx.id)
        return v || <span className="muted">-</span>
      }
    }
  }

  const onCellKey = (e: KeyboardEvent<HTMLButtonElement>, r: number, c: number, tx: Transaction, col: GridColumn) => {
    const moves: Record<string, [number, number]> = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] }
    if (moves[e.key]) {
      e.preventDefault()
      const [dr, dc] = moves[e.key]
      focusCell(Math.max(0, Math.min(view.length - 1, r + dr)), Math.max(0, Math.min(columns.length - 1, c + dc)))
    } else if ((e.key === 'Enter' || e.key === 'F2') && EDITABLE.includes(col)) {
      e.preventDefault()
      setEditing({ id: tx.id, col })
    }
  }

  /**
   * 편집 종료. move가 없으면(다른 곳을 눌러 blur로 끝난 경우) 사용자가 누른 곳의 포커스를 건드리지 않고,
   * 그 사이 다른 칸 편집을 시작했다면 그 편집도 닫지 않는다.
   */
  const stopEditing = (tx: Transaction, col: GridColumn, r: number, c: number, move?: Move) => {
    setEditing((cur) => (cur && cur.id === tx.id && cur.col === col ? null : cur))
    if (!move) return
    window.setTimeout(() => {
      if (move === 'next' || move === 'prev') {
        const candidates = columns.map((col, i) => ({ col, i })).filter(({ col }) => EDITABLE.includes(col))
        const target = move === 'next' ? candidates.find(({ i }) => i > c) : [...candidates].reverse().find(({ i }) => i < c)
        if (target) {
          setEditing({ id: tx.id, col: target.col })
          return
        }
      }
      focusCell(move === 'down' ? Math.min(view.length - 1, r + 1) : r, c)
    }, 0)
  }

  return (
    <div className="table-wrap" style={{ maxHeight: 'calc(100vh - 220px)', overflowY: 'auto' }}>
      <datalist id="cashbook-members">
        {refMaps.members.map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      <table className="sheet" aria-label="거래 목록">
        <thead>
          <tr>
            <th className="rownum">{selection ? <SelectAll rows={view} selection={selection} /> : '#'}</th>
            {columns.map((c) => (
              <th key={c} style={{ width: HEADERS[c].width, textAlign: HEADERS[c].num ? 'right' : 'left' }}>
                {HEADERS[c].label}
              </th>
            ))}
            <th style={{ width: 128 }}>상태</th>
          </tr>
        </thead>
        <tbody>
          {view.length === 0 && (
            <tr>
              <td colSpan={columns.length + 2} className="empty">
                {emptyText}
              </td>
            </tr>
          )}
          {view.map((tx, r) => {
            const overdue = tx.status === 'PLANNED' && tx.date < today
            const rowClass = [tx.status === 'PLANNED' ? 'planned' : '', overdue ? 'overdue' : '', tx.status === 'SKIPPED' ? 'planned' : '']
              .filter(Boolean)
              .join(' ')
            const checked = selection?.selected.has(tx.id) ?? false
            // 나눈 다른 쪽이 이 목록에 없으면(다른 장부) 아래에 보조 행으로 보여준다
            const hiddenParts = tx.splitWith.filter((p) => !visibleIds.has(p.id))
            return (
              <Fragment key={tx.id}>
                <tr className={checked ? `${rowClass} selected` : rowClass}>
                  <td className="rownum">
                    {selection ? (
                      <label className="row-check">
                        <input
                          type="checkbox"
                          aria-label={`${tx.description} ${won(tx.amount)}원 선택`}
                          checked={checked}
                          onChange={(e) => checkRow(r, e.target.checked, (e.nativeEvent as MouseEvent).shiftKey)}
                        />
                      </label>
                    ) : (
                      r + 1
                    )}
                  </td>
                  {columns.map((col, c) => {
                    const isEditing = editing?.id === tx.id && editing.col === col
                    return (
                      <td key={col} style={{ position: 'relative' }}>
                        {isEditing ? (
                          <CellEditor
                            tx={tx}
                            col={col}
                            refMaps={refMaps}
                            onCommit={(v, move) => {
                              commit(tx, col, v)
                              stopEditing(tx, col, r, c, move)
                            }}
                            onCancel={() => stopEditing(tx, col, r, c, 'stay')}
                          />
                        ) : (
                          <button
                            type="button"
                            className={`cell-btn ${HEADERS[col].num ? 'num' : ''}`}
                            data-cell={`${gridId}:${r}:${c}`}
                            onClick={() => EDITABLE.includes(col) && setEditing({ id: tx.id, col })}
                            onKeyDown={(e) => onCellKey(e, r, c, tx, col)}
                            title={col === 'description' || col === 'memo' ? String(display(tx, col) ?? '') : undefined}
                            style={{ cursor: EDITABLE.includes(col) ? 'text' : 'default' }}
                          >
                            {display(tx, col, view[r - 1])}
                          </button>
                        )}
                      </td>
                    )
                  })}
                  <td>
                    <div className="cell-actions">
                      <StatusBadges tx={tx} overdue={overdue} refMaps={refMaps} />
                      <span className="spacer" />
                      {tx.status === 'PLANNED' && (
                        <button className="btn sm" disabled={pending} onClick={() => run(CONFIRM_TX, { id: tx.id })} title="예정 금액·날짜로 확정">
                          확정
                        </button>
                      )}
                      {tx.status === 'PLANNED' && tx.recurringRuleId && (
                        <button className="btn sm ghost" disabled={pending} onClick={() => run(SKIP_TX, { id: tx.id })} title="이번 달은 건너뛰기">
                          건너뜀
                        </button>
                      )}
                      {tx.status === 'SKIPPED' && (
                        <button className="btn sm ghost" disabled={pending} onClick={() => run(RESTORE_TX, { id: tx.id })}>
                          복원
                        </button>
                      )}
                    </div>
                  </td>
                </tr>
                {hiddenParts.map((part) => (
                  <SplitSubRow key={part.id} part={part} tx={tx} columns={columns} refMaps={refMaps} />
                ))}
              </Fragment>
            )
          })}
          {newRow && !selection && (
            <NewRow
              // 달·장부·카드가 바뀌면 새 행 기본값(날짜 등)을 다시 잡는다
              key={`${newRow.date}:${newRow.ledgerId}:${newRow.paymentMethodId ?? ''}`}
              columns={columns}
              refMaps={refMaps}
              defaults={newRow}
            />
          )}
        </tbody>
        {footer && (
          <tfoot>
            <tr>
              <td colSpan={columns.length + 2} style={{ padding: '8px 10px', background: 'var(--surface-2)' }}>
                {footer}
              </td>
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  )
}

type Selection = NonNullable<Props['selection']>

/** 머리글 체크박스: 보이는 행을 모두 고르거나 모두 풀기 */
function SelectAll({ rows, selection }: { rows: Transaction[]; selection: Selection }) {
  const count = rows.filter((t) => selection.selected.has(t.id)).length
  const all = rows.length > 0 && count === rows.length
  return (
    <input
      type="checkbox"
      aria-label="모두 선택"
      checked={all}
      ref={(el) => {
        if (el) el.indeterminate = count > 0 && !all
      }}
      disabled={rows.length === 0}
      onChange={() => {
        const next = new Set(selection.selected)
        for (const t of rows) {
          if (all) next.delete(t.id)
          else next.add(t.id)
        }
        selection.onChange(next)
      }}
    />
  )
}

/** 목록 하단 합계: 지출(환불 차감)과 수입을 나눠 보여준다 */
export function PageTotals({ page, extra }: { page: TransactionPage; extra?: ReactNode }) {
  return (
    <div className="row wrap small">
      <span>{page.totalCount}건</span>
      <span className="spacer" />
      <span>
        지출 <b className="num">{won(page.confirmedTotal)}</b>
      </span>
      {page.plannedTotal !== 0 && (
        <span className="text-2">
          예정 지출 <span className="num">{won(page.plannedTotal)}</span>
        </span>
      )}
      {page.incomeTotal !== 0 && (
        <span className="good">
          수입 <span className="num">{won(page.incomeTotal)}</span>
        </span>
      )}
      {page.plannedIncomeTotal !== 0 && (
        <span className="text-2">
          예정 수입 <span className="num">{won(page.plannedIncomeTotal)}</span>
        </span>
      )}
      {extra}
    </div>
  )
}

/** 나눈 결제의 다른 장부 쪽. 편집은 그 장부 화면에서 한다. */
function SplitSubRow({ part, tx, columns, refMaps }: { part: SplitPart; tx: Transaction; columns: GridColumn[]; refMaps: RefMaps }) {
  const ledger = refMaps.ledgerById.get(part.ledgerId)?.name ?? '다른 장부'
  const cell = (col: GridColumn): ReactNode => {
    switch (col) {
      case 'amount':
        return won(part.amount)
      case 'category':
        return part.categoryId ? refMaps.categoryById.get(part.categoryId)?.name : <span className="muted">미분류</span>
      case 'ledger':
        return ledger
      case 'description':
        return (
          <>
            <span className="badge">{ledger}</span> 몫 · 결제 {won(splitTotal(tx))}원
          </>
        )
      default:
        return null
    }
  }
  return (
    <tr className="split-sub" aria-label={`${ledger}에 나눈 몫 ${won(part.amount)}원`}>
      <td className="rownum">↳</td>
      {columns.map((col) => (
        <td key={col}>
          <div className={`sub-cell ${HEADERS[col].num ? 'num' : ''}`}>{cell(col)}</div>
        </td>
      ))}
      <td />
    </tr>
  )
}

function StatusBadges({ tx, overdue, refMaps }: { tx: Transaction; overdue: boolean; refMaps: RefMaps }) {
  return (
    <>
      {isSplit(tx) && (
        <span
          className="badge split"
          title={`${[tx, ...tx.splitWith].map((p) => `${refMaps.ledgerById.get(p.ledgerId)?.name} ${won(p.amount)}`).join(' + ')} = 결제 ${won(splitTotal(tx))}원`}
        >
          분할
        </span>
      )}
      {tx.status === 'PLANNED' && <span className={`badge ${overdue ? 'warn' : 'planned'}`}>{overdue ? '! 미확정' : '예정'}</span>}
      {tx.status === 'SKIPPED' && <span className="badge">건너뜀</span>}
      {tx.source === 'API' && (
        <span className="badge" title={tx.rawText ?? SOURCE_LABEL.API}>
          자동
        </span>
      )}
      {tx.installmentMonths > 1 && <span className="badge">{tx.installmentMonths}개월</span>}
      {tx.excludeFromPerformance && (
        <span className="badge" title="카드 전월실적 제외">
          실적제외
        </span>
      )}
    </>
  )
}

function CellEditor({
  tx,
  col,
  refMaps,
  onCommit,
  onCancel,
}: {
  tx: Transaction
  col: GridColumn
  refMaps: RefMaps
  onCommit: (value: string | number[] | null, move?: Move) => void
  onCancel: () => void
}) {
  const initial = (() => {
    switch (col) {
      case 'date':
        return tx.date
      case 'amount':
        return String(tx.amount)
      case 'category':
        return tx.categoryId ? String(tx.categoryId) : ''
      case 'paymentMethod':
        return tx.paymentMethodId ? String(tx.paymentMethodId) : ''
      case 'ledger':
        return String(tx.ledgerId)
      case 'description':
        return tx.description
      case 'member':
        return tx.member ?? ''
      case 'memo':
        return tx.memo ?? ''
      default:
        return ''
    }
  })()
  const [value, setValue] = useState(initial)
  const done = useRef(false)
  const finish = (move?: Move, v: string = value) => {
    if (done.current) return
    done.current = true
    onCommit(v, move)
  }
  const keys = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      finish('down')
    } else if (e.key === 'Tab') {
      e.preventDefault()
      finish(e.shiftKey ? 'prev' : 'next')
    } else if (e.key === 'Escape') {
      e.preventDefault()
      done.current = true
      onCancel()
    }
  }

  if (col === 'benefits') {
    return (
      <BenefitEditor
        tx={tx}
        refMaps={refMaps}
        onCommit={(ids) => {
          done.current = true
          onCommit(ids, 'stay')
        }}
        onCancel={onCancel}
      />
    )
  }
  if (col === 'category' || col === 'paymentMethod' || col === 'ledger') {
    const options =
      col === 'category'
        ? [
            { value: '', label: '미분류' },
            ...refMaps.categoriesOf(tx.ledgerId).map((c) => ({ value: String(c.id), label: c.name })),
          ]
        : col === 'paymentMethod'
          ? [
              { value: '', label: '(없음)' },
              ...refMaps.paymentMethods
                .filter((p) => !p.archived || p.id === tx.paymentMethodId)
                .map((p) => ({ value: String(p.id), label: p.archived ? `${p.name} (보관)` : p.name })),
            ]
          : refMaps.ledgers.map((l) => ({ value: String(l.id), label: l.name }))
    return (
      <select
        className="cell-input"
        autoFocus
        value={value}
        onChange={(e) => {
          setValue(e.target.value)
          finish('stay', e.target.value)
        }}
        onBlur={() => finish()}
        onKeyDown={keys}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    )
  }
  if (col === 'description') {
    return (
      <DescriptionInput
        className="cell-input"
        value={value}
        onChange={setValue}
        ledgerId={tx.ledgerId}
        autoFocus
        onKeyDown={keys}
        onBlur={() => finish()}
      />
    )
  }
  return (
    <input
      className={`cell-input ${col === 'amount' ? 'num' : ''}`}
      autoFocus
      type={col === 'date' ? 'date' : 'text'}
      inputMode={col === 'amount' ? 'numeric' : undefined}
      list={col === 'member' ? 'cashbook-members' : undefined}
      value={value}
      onFocus={(e) => col !== 'date' && e.currentTarget.select()}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => finish()}
      onKeyDown={keys}
    />
  )
}

function BenefitEditor({
  tx,
  refMaps,
  onCommit,
  onCancel,
}: {
  tx: Transaction
  refMaps: RefMaps
  onCommit: (ids: number[]) => void
  onCancel: () => void
}) {
  const benefits = refMaps.paymentMethodById.get(tx.paymentMethodId ?? -1)?.benefits ?? []
  const [selected, setSelected] = useState<number[]>(tx.benefitIds)
  return (
    <div
      className="panel"
      style={{ position: 'absolute', zIndex: 20, padding: 10, minWidth: 220 }}
      onKeyDown={(e) => e.key === 'Escape' && onCancel()}
    >
      {benefits.length === 0 ? (
        <div className="small muted" style={{ marginBottom: 8 }}>
          이 결제수단에 등록된 혜택이 없습니다. 카드 상세에서 혜택을 추가하세요.
        </div>
      ) : (
        <div className="stack" style={{ gap: 6, marginBottom: 10 }}>
          <BenefitChecks benefits={benefits} selected={selected} onChange={setSelected} />
        </div>
      )}
      <div className="row">
        <button className="btn sm primary" autoFocus onClick={() => onCommit(selected)}>
          적용
        </button>
        <button className="btn sm" onClick={onCancel}>
          취소
        </button>
      </div>
    </div>
  )
}

interface NewRowState {
  date: string
  amount: string
  categoryId: string
  paymentMethodId: string
  description: string
  member: string
  memo: string
  ledgerId: string
  status: TxStatus
}

function NewRow({ columns, refMaps, defaults }: { columns: GridColumn[]; refMaps: RefMaps; defaults: NonNullable<Props['newRow']> }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const amountRef = useRef<HTMLInputElement>(null)
  const initialLedger = defaults.ledgerId ?? refMaps.defaultLedger?.id ?? null
  const blank = (keep?: Partial<NewRowState>): NewRowState => ({
    date: defaults.date,
    amount: '',
    categoryId: '',
    paymentMethodId: defaults.paymentMethodId ? String(defaults.paymentMethodId) : '',
    description: '',
    member: '',
    memo: '',
    ledgerId: initialLedger ? String(initialLedger) : '',
    status: 'CONFIRMED',
    ...keep,
  })
  const [s, setS] = useState<NewRowState>(() => blank())
  const set = (patch: Partial<NewRowState>) => setS((prev) => ({ ...prev, ...patch }))

  const ledgerForCategories = s.ledgerId ? Number(s.ledgerId) : null

  const pick = (sug: Suggestion) => {
    const ledgerColumn = columns.includes('ledger')
    const ledgerId = ledgerColumn ? String(sug.ledgerId) : s.ledgerId
    // 추천 카테고리가 이 행의 장부 것일 때만 쓴다 (다른 장부 카테고리는 저장이 거부된다)
    const sugCategory =
      sug.categoryId && refMaps.categoryById.get(sug.categoryId)?.ledgerId === Number(ledgerId) ? String(sug.categoryId) : ''
    set({
      description: sug.description,
      categoryId: ledgerColumn ? sugCategory : s.categoryId || sugCategory,
      paymentMethodId: s.paymentMethodId || (sug.paymentMethodId ? String(sug.paymentMethodId) : ''),
      ledgerId,
      member: s.member || sug.member || '',
    })
  }

  const submitting = useRef(false)
  const submit = async () => {
    // Enter를 연달아 눌러도 한 번만 저장한다
    if (submitting.current) return
    const amount = parseAmount(s.amount)
    if (amount == null) {
      toast('금액을 입력하세요', 'error')
      amountRef.current?.focus()
      return
    }
    if (!s.description.trim()) {
      toast('내용을 입력하세요', 'error')
      return
    }
    submitting.current = true
    try {
      const created = await run(CREATE_TX, {
        input: {
          ledgerId: s.ledgerId ? Number(s.ledgerId) : null,
          date: s.date,
          amount,
          categoryId: s.categoryId ? Number(s.categoryId) : null,
          paymentMethodId: s.paymentMethodId ? Number(s.paymentMethodId) : null,
          description: s.description.trim(),
          memo: s.memo || null,
          member: s.member || null,
          status: s.status,
        },
      })
      if (!created) return
      setS(blank({ date: s.date, paymentMethodId: s.paymentMethodId, ledgerId: s.ledgerId, status: s.status }))
      amountRef.current?.focus()
    } finally {
      submitting.current = false
    }
  }

  const enter = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      submit()
    }
  }

  const cell = (col: GridColumn): ReactNode => {
    switch (col) {
      case 'date':
        return <input className="cell-input" type="date" value={s.date} onChange={(e) => set({ date: e.target.value })} onKeyDown={enter} aria-label="날짜" />
      case 'amount':
        return (
          <input
            ref={amountRef}
            className="cell-input num"
            inputMode="numeric"
            placeholder="금액"
            value={s.amount}
            onChange={(e) => set({ amount: e.target.value })}
            onKeyDown={enter}
            aria-label="금액"
          />
        )
      case 'category':
        return (
          <select className="cell-input" value={s.categoryId} onChange={(e) => set({ categoryId: e.target.value })} onKeyDown={enter} aria-label="항목">
            <option value="">항목</option>
            {refMaps.categoriesOf(ledgerForCategories).map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        )
      case 'paymentMethod':
        return (
          <select className="cell-input" value={s.paymentMethodId} onChange={(e) => set({ paymentMethodId: e.target.value })} onKeyDown={enter} aria-label="결제수단">
            <option value="">결제수단</option>
            {refMaps.activePaymentMethods.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        )
      case 'description':
        return (
          <DescriptionInput
            className="cell-input"
            value={s.description}
            onChange={(v) => set({ description: v })}
            onPick={pick}
            ledgerId={columns.includes('ledger') ? null : ledgerForCategories}
            onKeyDown={enter}
          />
        )
      case 'member':
        return <input className="cell-input" list="cashbook-members" value={s.member} onChange={(e) => set({ member: e.target.value })} onKeyDown={enter} aria-label="구성원" />
      case 'memo':
        return <input className="cell-input" value={s.memo} placeholder="메모" onChange={(e) => set({ memo: e.target.value })} onKeyDown={enter} aria-label="메모" />
      case 'ledger':
        return (
          <select className="cell-input" value={s.ledgerId} onChange={(e) => set({ ledgerId: e.target.value, categoryId: '' })} aria-label="장부">
            {refMaps.ledgers.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name}
              </option>
            ))}
          </select>
        )
      default:
        return null
    }
  }

  return (
    <tr className="new-row">
      <td className="rownum">＋</td>
      {columns.map((col) => (
        <Fragment key={col}>
          <td>{cell(col)}</td>
        </Fragment>
      ))}
      <td>
        <div className="cell-actions">
          <select
            className="input"
            style={{ height: 26, padding: '0 4px', fontSize: 12 }}
            value={s.status}
            onChange={(e) => set({ status: e.target.value as TxStatus })}
            aria-label="상태"
          >
            <option value="CONFIRMED">확정</option>
            <option value="PLANNED">예정</option>
          </select>
          <button className="btn sm primary" onClick={submit} disabled={pending}>
            추가
          </button>
        </div>
      </td>
    </tr>
  )
}
