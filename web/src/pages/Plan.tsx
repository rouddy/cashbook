import { useState, type FormEvent } from 'react'

import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import {
  CREATE_CATEGORY,
  CREATE_PLAN_ITEM,
  CREATE_RULE,
  DELETE_CATEGORY,
  DELETE_PLAN_ITEM,
  DELETE_RULE,
  PLAN_ITEMS_QUERY,
  RECURRING_QUERY,
  REORDER_CATEGORIES,
  SUMMARY_QUERY,
  UPDATE_CATEGORY,
  UPDATE_PLAN_ITEM,
  UPDATE_RULE,
} from '../api/queries'
import type { Category, CategoryKind, MonthlySummary, PlanItem, PlanItemKind, RecurringRule } from '../api/types'
import BenefitChecks from '../components/BenefitChecks'
import { ErrorBox, Loading, Panel } from '../components/Common'
import LedgerTabs from '../components/LedgerTabs'
import Modal from '../components/Modal'
import { useToast } from '../components/Toast'
import { parseAmount, PLAN_KIND_LABEL, shortDate, won } from '../lib/format'
import { currentMonth, monthLabel, useQueryParam } from '../lib/month'
import { transferLabel } from '../lib/transfer'

export default function Plan() {
  const { ref, error } = useRefData()
  const [ledgerParam, setLedgerParam] = useQueryParam('ledger', 'default')
  if (error) return <ErrorBox error={error} />
  if (!ref) return <Loading />
  const ledgerId = ledgerParam === 'default' ? ref.defaultLedger?.id ?? ref.ledgers[0]?.id : Number(ledgerParam)
  if (!ledgerId) return <div className="panel empty">장부가 없습니다. 설정에서 장부를 만드세요.</div>
  return (
    <div className="stack">
      <div className="page-head">
        <h1>지출 계획</h1>
        <LedgerTabs ledgers={ref.ledgers} value={String(ledgerId)} onChange={setLedgerParam} allowAll={false} />
      </div>
      <PlanBody key={ledgerId} ledgerId={ledgerId} refMaps={ref} />
    </div>
  )
}

function PlanBody({ ledgerId, refMaps }: { ledgerId: number; refMaps: RefMaps }) {
  const month = currentMonth()
  const items = useGql<{ planItems: PlanItem[] }>(['plan', ledgerId], PLAN_ITEMS_QUERY, { ledgerId })
  const summary = useGql<{ monthlySummary: MonthlySummary }>(['summary', month, ledgerId, '6', 'plan'], SUMMARY_QUERY, {
    month,
    ledgerId,
    averageMonths: 6,
    historyMonths: 1,
  })
  const s = summary.data?.monthlySummary
  const planItems = items.data?.planItems ?? []
  const allocations = planItems.filter((p) => p.kind === 'ALLOCATION').reduce((a, p) => a + p.amount, 0)
  const categories = refMaps.categoriesOf(ledgerId)
  const budgetTotal = categories.filter((c) => c.kind === 'EXPENSE').reduce((a, c) => a + (c.monthlyBudget ?? 0), 0)
  // 이번 달 확정 + 예정 (정기수익·이체는 반복 규칙이 예정으로 넣는다)
  const income = s ? s.income + s.plannedIncome : 0
  const transferIn = s ? s.transferIn + s.plannedTransferIn : 0
  const transferOut = s ? s.transferOut + s.plannedTransferOut : 0
  const leftover = income + transferIn - transferOut - allocations - budgetTotal

  return (
    <>
      <div className="kpis">
        <div className="panel stat">
          <div className="stat-label">{monthLabel(month)} 들어오는 돈</div>
          <div className="stat-value">{won(income + transferIn)}원</div>
          <div className="stat-delta text-2">
            수입 {won(income)}원{transferIn !== 0 && ` · 받은 이체 ${won(transferIn)}원`} · 예정 포함
          </div>
        </div>
        <div className="panel stat">
          <div className="stat-label">다른 장부로 이체 · 배분</div>
          <div className="stat-value">{won(transferOut + allocations)}원</div>
          <div className="stat-delta text-2">
            이체 {won(transferOut)}원 · 배분 {won(allocations)}원
          </div>
        </div>
        <div className="panel stat">
          <div className="stat-label">카테고리 예산 합계</div>
          <div className="stat-value">{won(budgetTotal)}원</div>
        </div>
        <div className="panel stat">
          <div className="stat-label">계획상 남는 돈</div>
          <div className={`stat-value ${leftover < 0 ? 'bad' : ''}`}>{won(leftover)}원</div>
          {s?.balance != null && (
            <div className="stat-delta text-2">
              {monthLabel(month)} 실제 월잔액 {won(s.balance)}원 (확정 기준)
            </div>
          )}
        </div>
      </div>
      <div className="grid-2" style={{ alignItems: 'start' }}>
        <CategoryBudgets ledgerId={ledgerId} refMaps={refMaps} summary={s} />
        <PlanItems ledgerId={ledgerId} items={planItems} loading={items.isLoading} refMaps={refMaps} />
      </div>
      <RecurringRules ledgerId={ledgerId} refMaps={refMaps} />
    </>
  )
}

function CategoryBudgets({ ledgerId, refMaps, summary }: { ledgerId: number; refMaps: RefMaps; summary?: MonthlySummary }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const [newName, setNewName] = useState('')
  const [newKind, setNewKind] = useState<CategoryKind>('EXPENSE')
  const [showArchived, setShowArchived] = useState(false)
  const all = refMaps.categories
    .filter((c) => c.ledgerId === ledgerId && (showArchived || !c.archived))
    .sort((a, b) => Number(a.archived) - Number(b.archived) || a.sortOrder - b.sortOrder)
  const stat = new Map((summary?.categories ?? []).map((c) => [c.categoryId, c]))

  const patch = (c: Category, input: Record<string, unknown>) => run(UPDATE_CATEGORY, { id: c.id, input })

  const add = async (e: FormEvent) => {
    e.preventDefault()
    if (!newName.trim()) return
    if (await run(CREATE_CATEGORY, { input: { ledgerId, name: newName.trim(), kind: newKind } })) setNewName('')
  }

  const remove = async (c: Category) => {
    if (!window.confirm(`'${c.name}' 카테고리를 삭제할까요? 쓰인 적이 있으면 보관 처리됩니다.`)) return
    const r = await run<{ deleteCategory: { deleted: boolean } }>(DELETE_CATEGORY, { id: c.id })
    if (!r) return
    toast(r.deleteCategory.deleted ? '삭제했습니다' : '거래가 있어 보관했습니다')
  }

  const move = async (c: Category, dir: -1 | 1) => {
    const ids = all.filter((x) => !x.archived).map((x) => x.id)
    const i = ids.indexOf(c.id)
    const j = i + dir
    if (i < 0 || j < 0 || j >= ids.length) return
    ;[ids[i], ids[j]] = [ids[j], ids[i]]
    await run(REORDER_CATEGORIES, { ids })
  }

  return (
    <Panel
      title="카테고리 예산"
      sub={`이번 달 = 확정+예정, 평균 = 지난 ${summary?.averageMonthsUsed ?? 6}개월`}
      actions={
        <label className="check small">
          <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
          보관 포함
        </label>
      }
      flush
    >
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>카테고리</th>
              <th className="num">월 예산</th>
              <th className="num">이번 달</th>
              <th className="num">평균</th>
              <th title="고정비 (대출·보험·관리비 등)">고정비</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {all.map((c) => {
              const st = stat.get(c.id)
              const used = st ? st.confirmed + st.planned : 0
              const over = c.monthlyBudget != null && used > c.monthlyBudget
              return (
                <tr key={c.id} style={c.archived ? { opacity: 0.55 } : undefined}>
                  <td>
                    <InlineText value={c.name} onSave={(v) => patch(c, { name: v })} />
                    {c.kind === 'INCOME' && <span className="badge good">수입</span>}
                  </td>
                  <td className="num">
                    <InlineAmount value={c.monthlyBudget} onSave={(v) => patch(c, { monthlyBudget: v })} />
                  </td>
                  <td className={`num ${over ? 'bad' : ''}`}>{won(used)}</td>
                  <td className="num text-2">{st ? won(st.average) : '-'}</td>
                  <td>
                    <input type="checkbox" checked={c.isFixed} disabled={pending} onChange={(e) => patch(c, { isFixed: e.target.checked })} aria-label={`${c.name} 고정비`} />
                  </td>
                  <td className="nowrap">
                    {c.archived ? (
                      <button className="btn sm ghost" onClick={() => patch(c, { archived: false })}>
                        복원
                      </button>
                    ) : (
                      <>
                        <button className="btn sm ghost icon" aria-label="위로" onClick={() => move(c, -1)}>
                          ↑
                        </button>
                        <button className="btn sm ghost icon" aria-label="아래로" onClick={() => move(c, 1)}>
                          ↓
                        </button>
                        <button className="btn sm ghost icon danger" aria-label="삭제" onClick={() => remove(c)}>
                          ✕
                        </button>
                      </>
                    )}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      <form className="row" style={{ padding: 12 }} onSubmit={add}>
        <input className="input" placeholder="새 카테고리" value={newName} onChange={(e) => setNewName(e.target.value)} />
        <select className="input" value={newKind} onChange={(e) => setNewKind(e.target.value as CategoryKind)}>
          <option value="EXPENSE">지출</option>
          <option value="INCOME">수입</option>
        </select>
        <button className="btn" disabled={!newName.trim() || pending}>
          추가
        </button>
      </form>
    </Panel>
  )
}

function InlineText({ value, onSave }: { value: string; onSave: (v: string) => void }) {
  const [editing, setEditing] = useState(false)
  const [v, setV] = useState(value)
  if (!editing)
    return (
      <button className="cell-btn" style={{ display: 'inline', width: 'auto', padding: '0 4px', height: 'auto', lineHeight: 1.5 }} onClick={() => { setV(value); setEditing(true) }}>
        {value}
      </button>
    )
  const finish = () => {
    setEditing(false)
    if (v.trim() && v.trim() !== value) onSave(v.trim())
  }
  return <input className="input" autoFocus value={v} onChange={(e) => setV(e.target.value)} onBlur={finish} onKeyDown={(e) => { if (e.key === 'Enter') finish(); if (e.key === 'Escape') setEditing(false) }} style={{ width: 120 }} />
}

function InlineAmount({ value, onSave }: { value: number | null; onSave: (v: number | null) => void }) {
  const [editing, setEditing] = useState(false)
  const [v, setV] = useState(value == null ? '' : String(value))
  if (!editing)
    return (
      <button className="cell-btn num" style={{ height: 'auto', lineHeight: 1.5 }} onClick={() => { setV(value == null ? '' : String(value)); setEditing(true) }}>
        {value == null ? <span className="muted">없음</span> : won(value)}
      </button>
    )
  const finish = () => {
    setEditing(false)
    const parsed = v.trim() === '' ? null : parseAmount(v)
    if (v.trim() !== '' && parsed == null) return
    if (parsed !== value) onSave(parsed)
  }
  return <input className="input num" autoFocus inputMode="numeric" value={v} onChange={(e) => setV(e.target.value)} onBlur={finish} onKeyDown={(e) => { if (e.key === 'Enter') finish(); if (e.key === 'Escape') setEditing(false) }} style={{ width: 110 }} />
}

function PlanItems({ ledgerId, items, loading, refMaps }: { ledgerId: number; items: PlanItem[]; loading: boolean; refMaps: RefMaps }) {
  const { run, pending } = useMutate()
  const [kind, setKind] = useState<PlanItemKind>('ALLOCATION')
  const [name, setName] = useState('')
  const [amount, setAmount] = useState('')
  const [member, setMember] = useState('')

  const add = async (e: FormEvent) => {
    e.preventDefault()
    const a = parseAmount(amount)
    if (!name.trim() || a == null) return
    const created = await run(CREATE_PLAN_ITEM, { input: { ledgerId, kind, name: name.trim(), amount: a, member: kind === 'MEMBER_ALLOWANCE' ? member || null : null } })
    if (!created) return
    setName('')
    setAmount('')
  }

  const save = (p: PlanItem, patch: Partial<PlanItem>) =>
    run(UPDATE_PLAN_ITEM, {
      id: p.id,
      input: { ledgerId: p.ledgerId, kind: p.kind, name: p.name, amount: p.amount, member: p.member, sortOrder: p.sortOrder, ...patch },
    })

  return (
    <Panel
      title="배분 · 구성원 용돈"
      sub="월잔액 = 수입 + 받은 이체 − 소비 − 보낸 이체 − 배분. 월급은 정기수익 반복 수입으로, 다른 장부로 보내는 돈은 반복 이체로 아래에 등록하세요. 구성원 용돈은 그 사람 표시 지출과 비교만 합니다"
      flush
    >
      {loading ? (
        <Loading />
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>종류</th>
              <th>이름</th>
              <th className="num">금액</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {items.map((p) => (
              <tr key={p.id}>
                <td>
                  <span className="badge">{PLAN_KIND_LABEL[p.kind]}</span>
                </td>
                <td>
                  <InlineText value={p.name} onSave={(v) => save(p, { name: v })} />
                  {p.member && <span className="small muted"> ({p.member})</span>}
                </td>
                <td className="num">
                  <InlineAmount value={p.amount} onSave={(v) => v != null && save(p, { amount: v })} />
                </td>
                <td>
                  <button className="btn sm ghost icon danger" aria-label="삭제" disabled={pending} onClick={() => window.confirm(`'${p.name}' 항목을 삭제할까요?`) && run(DELETE_PLAN_ITEM, { id: p.id })}>
                    ✕
                  </button>
                </td>
              </tr>
            ))}
            {items.length === 0 && (
              <tr>
                <td colSpan={4} className="empty">
                  저축처럼 매달 떼어 두는 금액이나 구성원 용돈을 추가하세요
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
      <form className="row wrap" style={{ padding: 12 }} onSubmit={add}>
        <select className="input" value={kind} onChange={(e) => setKind(e.target.value as PlanItemKind)}>
          <option value="ALLOCATION">배분</option>
          <option value="MEMBER_ALLOWANCE">구성원 용돈</option>
        </select>
        <input className="input" placeholder="이름 (예: 저축)" value={name} onChange={(e) => setName(e.target.value)} style={{ width: 130 }} />
        {kind === 'MEMBER_ALLOWANCE' && (
          <input className="input" list="plan-members" placeholder="구성원" value={member} onChange={(e) => setMember(e.target.value)} style={{ width: 90 }} />
        )}
        <datalist id="plan-members">
          {refMaps.members.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
        <input className="input num" inputMode="numeric" placeholder="금액" value={amount} onChange={(e) => setAmount(e.target.value)} style={{ width: 110 }} />
        <button className="btn" disabled={pending || !name.trim() || parseAmount(amount) == null}>
          추가
        </button>
      </form>
    </Panel>
  )
}

function RecurringRules({ ledgerId, refMaps }: { ledgerId: number; refMaps: RefMaps }) {
  const q = useGql<{ recurringRules: RecurringRule[] }>(['recurring', ledgerId], RECURRING_QUERY, { ledgerId })
  const { run, pending } = useMutate()
  const [editing, setEditing] = useState<RecurringRule | 'new' | null>(null)
  const rules = q.data?.recurringRules ?? []
  const monthly = rules.filter((r) => r.active && r.intervalMonths === 1)
  const sumOf = (pred: (r: RecurringRule) => boolean) => monthly.filter(pred).reduce((a, r) => a + r.amount, 0)
  const isIncome = (r: RecurringRule) => refMaps.categoryById.get(r.categoryId ?? -1)?.kind === 'INCOME'
  const totals = [
    ['지출', sumOf((r) => r.transferLedgerId == null && !isIncome(r))],
    ['수입', sumOf((r) => r.transferLedgerId == null && isIncome(r))],
    ['보내는 이체', sumOf((r) => r.transferLedgerId != null && r.ledgerId === ledgerId)],
    ['받는 이체', sumOf((r) => r.transferLedgerId === ledgerId)],
  ] as const
  const totalText = totals
    .filter(([, v]) => v !== 0)
    .map(([k, v]) => `${k} ${won(v)}원`)
    .join(' · ')
  return (
    <Panel
      title="반복 지출 · 수입 · 장부 이체 (예정 자동 생성)"
      sub={`매달 정해진 날 예정 거래를 만듭니다 (이번 달 + ${refMaps.appInfo.planMonthsAhead}개월).${totalText ? ` 매월 ${totalText}` : ''}`}
      actions={
        <button className="btn sm primary" onClick={() => setEditing('new')}>
          + 반복 항목 추가
        </button>
      }
      flush
    >
      {q.isLoading ? (
        <Loading />
      ) : q.error ? (
        <ErrorBox error={q.error} />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>날짜</th>
                <th>내용</th>
                <th className="num">금액</th>
                <th>항목</th>
                <th>결제수단</th>
                <th>기간</th>
                <th>다음</th>
                <th>사용</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rules.map((r) => (
                <tr key={r.id} style={r.active ? undefined : { opacity: 0.55 }}>
                  <td className="nowrap">
                    {r.intervalMonths === 1 ? '매월' : `${r.intervalMonths}개월마다`} {r.dayOfMonth >= 31 ? '말일' : `${r.dayOfMonth}일`}
                  </td>
                  <td>
                    {r.description}
                    {matchLabel(r) && <div className="small muted">매칭: {matchLabel(r)}</div>}
                  </td>
                  <td className={`num ${isIncome(r) || r.transferLedgerId === ledgerId ? 'good' : ''}`}>{won(r.amount)}</td>
                  <td>{transferLabel(r, refMaps, ledgerId) ?? refMaps.categoryById.get(r.categoryId ?? -1)?.name ?? '-'}</td>
                  <td>
                    {refMaps.paymentMethodById.get(r.paymentMethodId ?? -1)?.name ?? '-'}
                    {ruleBenefitNames(r, refMaps) && <div className="small muted">혜택: {ruleBenefitNames(r, refMaps)}</div>}
                  </td>
                  <td className="small nowrap">
                    {r.startMonth} ~ {r.endMonth ?? ''}
                  </td>
                  <td className="nowrap">{r.nextDate ? shortDate(r.nextDate) : '-'}</td>
                  <td>
                    <input
                      type="checkbox"
                      checked={r.active}
                      disabled={pending}
                      aria-label={`${r.description} 사용`}
                      onChange={(e) => run(UPDATE_RULE, { id: r.id, input: ruleInput(r, { active: e.target.checked }) })}
                    />
                  </td>
                  <td className="nowrap">
                    <button className="btn sm ghost" onClick={() => setEditing(r)}>
                      편집
                    </button>
                    <button
                      className="btn sm ghost icon danger"
                      aria-label="삭제"
                      onClick={() => window.confirm(`'${r.description}' 반복 항목을 삭제할까요? 이번 달 이후 예정 항목도 지워집니다.`) && run(DELETE_RULE, { id: r.id })}
                    >
                      ✕
                    </button>
                  </td>
                </tr>
              ))}
              {rules.length === 0 && (
                <tr>
                  <td colSpan={9} className="empty">
                    보험료·관리비처럼 매달 나가는 돈, 월급(정기수익), 다른 장부로 보내는 돈을 등록하면 예정으로 보여줍니다
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
      {editing && <RuleModal rule={editing === 'new' ? undefined : editing} ledgerId={ledgerId} refMaps={refMaps} onClose={() => setEditing(null)} />}
    </Panel>
  )
}

/** 자동 입력 매칭 조건 요약 (키워드 · 금액 범위) */
function matchLabel(r: RecurringRule): string | null {
  const { matchAmountMin: min, matchAmountMax: max } = r
  const range =
    min != null && max != null
      ? min === max
        ? `${won(min)}원만`
        : `${won(min)}~${won(max)}원`
      : min != null
        ? `${won(min)}원 이상`
        : max != null
          ? `${won(max)}원 이하`
          : null
  return [r.matchKeyword, range].filter(Boolean).join(' · ') || null
}

/** 규칙에 골라 둔 카드 혜택 이름 "10% 할인, 주유" (없으면 null) */
function ruleBenefitNames(r: RecurringRule, refMaps: RefMaps): string | null {
  const benefits = refMaps.paymentMethodById.get(r.paymentMethodId ?? -1)?.benefits ?? []
  const names = r.benefitIds.map((id) => benefits.find((b) => b.id === id)?.name).filter(Boolean)
  return names.join(', ') || null
}

function ruleInput(r: RecurringRule, patch: Partial<RecurringRule> = {}) {
  const x = { ...r, ...patch }
  return {
    ledgerId: x.ledgerId,
    description: x.description,
    amount: x.amount,
    categoryId: x.categoryId,
    paymentMethodId: x.paymentMethodId,
    dayOfMonth: x.dayOfMonth,
    intervalMonths: x.intervalMonths,
    startMonth: x.startMonth,
    endMonth: x.endMonth,
    member: x.member,
    memo: x.memo,
    matchKeyword: x.matchKeyword,
    matchAmountMin: x.matchAmountMin,
    matchAmountMax: x.matchAmountMax,
    benefitIds: x.benefitIds,
    excludeFromPerformance: x.excludeFromPerformance,
    transferLedgerId: x.transferLedgerId,
    active: x.active,
  }
}

function RuleModal({ rule, ledgerId, refMaps, onClose }: { rule?: RecurringRule; ledgerId: number; refMaps: RefMaps; onClose: () => void }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const [description, setDescription] = useState(rule?.description ?? '')
  const [amount, setAmount] = useState(rule ? String(rule.amount) : '')
  const [categoryId, setCategoryId] = useState(rule?.categoryId ? String(rule.categoryId) : '')
  const [paymentMethodId, setPaymentMethodId] = useState(rule?.paymentMethodId ? String(rule.paymentMethodId) : '')
  const [dayOfMonth, setDayOfMonth] = useState(String(rule?.dayOfMonth ?? 25))
  const [intervalMonths, setIntervalMonths] = useState(String(rule?.intervalMonths ?? 1))
  const [startMonth, setStartMonth] = useState(rule?.startMonth ?? currentMonth())
  const [endMonth, setEndMonth] = useState(rule?.endMonth ?? '')
  const [member, setMember] = useState(rule?.member ?? '')
  const [memo, setMemo] = useState(rule?.memo ?? '')
  const [matchKeyword, setMatchKeyword] = useState(rule?.matchKeyword ?? '')
  const [matchMin, setMatchMin] = useState(rule?.matchAmountMin != null ? String(rule.matchAmountMin) : '')
  const [matchMax, setMatchMax] = useState(rule?.matchAmountMax != null ? String(rule.matchAmountMax) : '')
  const [excludePerf, setExcludePerf] = useState(rule?.excludeFromPerformance ?? false)
  // 혜택은 카드에 딸려 있어 결제수단을 바꾸면 비운다
  const [benefitIds, setBenefitIds] = useState<number[]>(rule?.benefitIds ?? [])
  const pm = paymentMethodId ? refMaps.paymentMethodById.get(Number(paymentMethodId)) : undefined
  const fromLedger = rule?.ledgerId ?? ledgerId
  const otherLedgers = refMaps.ledgers.filter((l) => l.id !== fromLedger)
  // 지출·수입은 항목으로, 장부 이체는 받는 장부로 정한다
  const [isTransfer, setIsTransfer] = useState(rule?.transferLedgerId != null)
  const [transferTo, setTransferTo] = useState(String(rule?.transferLedgerId ?? otherLedgers[0]?.id ?? ''))

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    const a = parseAmount(amount)
    if (a == null || !description.trim()) {
      toast('내용과 금액을 입력하세요', 'error')
      return
    }
    if (isTransfer && !transferTo) {
      toast('받는 장부를 고르세요', 'error')
      return
    }
    // 금액 범위는 칸마다 비워 둘 수 있다 (비우면 그쪽 끝 없음)
    const matchMinValue = matchMin.trim() ? parseAmount(matchMin) : null
    const matchMaxValue = matchMax.trim() ? parseAmount(matchMax) : null
    if (!isTransfer && ((matchMin.trim() && matchMinValue == null) || (matchMax.trim() && matchMaxValue == null))) {
      toast('자동입력 금액 범위는 숫자로 입력하세요', 'error')
      return
    }
    const input = {
      ledgerId: fromLedger,
      description: description.trim(),
      amount: a,
      categoryId: categoryId && !isTransfer ? Number(categoryId) : null,
      paymentMethodId: paymentMethodId && !isTransfer ? Number(paymentMethodId) : null,
      transferLedgerId: isTransfer ? Number(transferTo) : null,
      dayOfMonth: Number(dayOfMonth),
      intervalMonths: Number(intervalMonths),
      startMonth,
      endMonth: endMonth || null,
      member: (!isTransfer && member) || null,
      memo: memo || null,
      matchKeyword: (!isTransfer && matchKeyword) || null,
      matchAmountMin: isTransfer ? null : matchMinValue,
      matchAmountMax: isTransfer ? null : matchMaxValue,
      benefitIds: isTransfer || !pm ? [] : benefitIds,
      excludeFromPerformance: excludePerf && !isTransfer,
      active: rule?.active ?? true,
    }
    const saved = rule ? await run(UPDATE_RULE, { id: rule.id, input }) : await run(CREATE_RULE, { input })
    if (!saved) return
    toast('저장했습니다. 이번 달 이후 예정 항목을 새로 만들었습니다')
    onClose()
  }

  return (
    <Modal
      title={rule ? '반복 항목 편집' : '반복 항목 추가'}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            취소
          </button>
          <button className="btn primary" form="rule-form" disabled={pending}>
            저장
          </button>
        </>
      }
    >
      <form id="rule-form" className="form-grid" onSubmit={submit}>
        <div className="field" style={{ gridColumn: '1 / -1' }}>
          종류
          <div className="segmented" role="group" aria-label="반복 종류">
            <button type="button" className={isTransfer ? '' : 'on'} onClick={() => setIsTransfer(false)}>
              지출 · 수입
            </button>
            <button
              type="button"
              className={isTransfer ? 'on' : ''}
              disabled={otherLedgers.length === 0}
              title={otherLedgers.length === 0 ? '설정에서 장부를 더 만드세요' : '매달 정해진 금액을 다른 장부로 옮깁니다'}
              onClick={() => setIsTransfer(true)}
            >
              장부 이체
            </button>
          </div>
          <span className="hint">
            {isTransfer
              ? `${refMaps.ledgerById.get(fromLedger)?.name}에서 나가고 받는 장부로 들어갑니다. 소비·수입 합계에는 넣지 않습니다`
              : '월급은 수입 항목(정기수익)을 고르면 예정 수입이 됩니다'}
          </span>
        </div>
        <label className="field">
          내용 *
          <input
            className="input"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={isTransfer ? '예: 여행적금' : '예: 아파트관리비'}
            autoFocus
          />
        </label>
        <label className="field">
          금액 *
          <input className="input num" inputMode="numeric" value={amount} onChange={(e) => setAmount(e.target.value)} />
        </label>
        {isTransfer ? (
          <label className="field">
            받는 장부
            <select className="input" value={transferTo} onChange={(e) => setTransferTo(e.target.value)}>
              {otherLedgers.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <>
            <label className="field">
              항목
              <select className="input" value={categoryId} onChange={(e) => setCategoryId(e.target.value)}>
                <option value="">미분류</option>
                {refMaps.categoriesOf(fromLedger).map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                    {c.kind === 'INCOME' ? ' (수입)' : ''}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              결제수단
              <select
                className="input"
                value={paymentMethodId}
                onChange={(e) => {
                  setPaymentMethodId(e.target.value)
                  setBenefitIds([])
                }}
              >
                <option value="">선택 안 함</option>
                {refMaps.activePaymentMethods.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            {pm && pm.benefits.length > 0 && (
              <div className="field-group" role="group" aria-label={`${pm.name} 혜택`} style={{ gridColumn: '1 / -1' }}>
                {pm.name} 혜택
                <div className="row wrap" style={{ gap: 10 }}>
                  <BenefitChecks benefits={pm.benefits} selected={benefitIds} onChange={setBenefitIds} />
                </div>
                <span className="hint">예정 거래에 미리 붙여 둡니다. 결제수단을 바꾸면 비워집니다</span>
              </div>
            )}
          </>
        )}
        <label className="field">
          날짜
          <select className="input" value={dayOfMonth} onChange={(e) => setDayOfMonth(e.target.value)}>
            {Array.from({ length: 30 }, (_, i) => i + 1).map((d) => (
              <option key={d} value={d}>
                {d}일
              </option>
            ))}
            <option value={31}>말일</option>
          </select>
        </label>
        <label className="field">
          반복
          <select className="input" value={intervalMonths} onChange={(e) => setIntervalMonths(e.target.value)}>
            <option value={1}>매월</option>
            <option value={2}>2개월마다</option>
            <option value={3}>3개월마다</option>
            <option value={6}>6개월마다</option>
            <option value={12}>매년</option>
          </select>
        </label>
        <label className="field">
          시작 월
          <input className="input" type="month" value={startMonth} onChange={(e) => setStartMonth(e.target.value)} />
        </label>
        <label className="field">
          종료 월 (선택)
          <input className="input" type="month" value={endMonth} onChange={(e) => setEndMonth(e.target.value)} />
        </label>
        {!isTransfer && (
          <>
            <label className="field">
              구성원
              <input className="input" value={member} onChange={(e) => setMember(e.target.value)} />
            </label>
            <label className="field">
              자동입력 매칭 키워드
              <input className="input" value={matchKeyword} onChange={(e) => setMatchKeyword(e.target.value)} placeholder="예: 관리비" />
              <span className="hint">정하면 승인 문자 가맹점에 이 말이 있을 때만 확정 (쉼표로 여러 개). 금액 범위가 없으면 금액이 달라도 확정</span>
            </label>
            <div className="field-group" role="group" aria-label="자동입력 금액 범위" style={{ gridColumn: '1 / -1' }}>
              자동입력 금액 범위
              <div className="row">
                <input
                  className="input num"
                  inputMode="numeric"
                  aria-label="금액 범위 시작"
                  placeholder="시작"
                  value={matchMin}
                  onChange={(e) => setMatchMin(e.target.value)}
                  style={{ width: 130 }}
                />
                <span className="muted">~</span>
                <input
                  className="input num"
                  inputMode="numeric"
                  aria-label="금액 범위 끝"
                  placeholder="끝"
                  value={matchMax}
                  onChange={(e) => setMatchMax(e.target.value)}
                  style={{ width: 130 }}
                />
                <button
                  type="button"
                  className="btn sm ghost"
                  disabled={parseAmount(amount) == null}
                  onClick={() => {
                    setMatchMin(amount)
                    setMatchMax(amount)
                  }}
                >
                  이 금액만
                </button>
              </div>
              <span className="hint">
                정하면 실제 금액이 이 범위 안(양끝 포함)일 때만 확정. 시작과 끝을 같게 하면 그 금액일 때만, 한쪽만 써도 됩니다. 비우면 금액이 예정과 같거나
                내용·키워드가 맞을 때 확정
              </span>
            </div>
          </>
        )}
        <label className="field" style={{ gridColumn: '1 / -1' }}>
          메모
          <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        {!isTransfer && (
          <label className="check">
            <input type="checkbox" checked={excludePerf} onChange={(e) => setExcludePerf(e.target.checked)} />
            카드 전월실적에서 제외 (관리비·보험 등)
          </label>
        )}
      </form>
    </Modal>
  )
}
