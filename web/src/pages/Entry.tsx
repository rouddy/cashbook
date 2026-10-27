import { useState, type FormEvent } from 'react'
import { Link } from 'react-router-dom'

import { gql } from '../api/client'
import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import { CREATE_TX, INGEST, PARSE_MESSAGE_QUERY, TRANSACTIONS_QUERY } from '../api/queries'
import type { ParsedMessage, Suggestion, Transaction, TransactionPage, TxStatus } from '../api/types'
import { ErrorBox, Loading, Panel } from '../components/Common'
import DescriptionInput from '../components/DescriptionInput'
import { useToast } from '../components/Toast'
import { parseAmount, shortDate, STATUS_LABEL, won } from '../lib/format'
import { addDays, localToday } from '../lib/month'

interface FormState {
  date: string
  amount: string
  refund: boolean
  description: string
  ledgerId: number | null
  categoryId: number | null
  paymentMethodId: number | null
  member: string
  memo: string
  installmentMonths: number
  status: TxStatus
  benefitIds: number[]
  excludeFromPerformance: boolean
}

function emptyForm(ref: RefMaps, keep?: Partial<FormState>): FormState {
  return {
    date: localToday(),
    amount: '',
    refund: false,
    description: '',
    ledgerId: ref.defaultLedger?.id ?? null,
    categoryId: null,
    paymentMethodId: null,
    member: '',
    memo: '',
    installmentMonths: 1,
    status: 'CONFIRMED',
    benefitIds: [],
    excludeFromPerformance: false,
    ...keep,
  }
}

/** 문자에서 뽑은 카드 정보로 결제수단 찾기 (끝 4자리 → 카드사가 하나뿐일 때) */
function matchCard(ref: RefMaps, p: ParsedMessage): number | null {
  const cards = ref.activePaymentMethods.filter((c) => c.isCard)
  if (p.last4) {
    const byLast4 = cards.find((c) => c.last4 === p.last4)
    if (byLast4) return byLast4.id
  }
  if (p.issuer) {
    const byIssuer = cards.filter((c) => c.issuer && (c.issuer.includes(p.issuer!) || p.issuer!.includes(c.issuer)))
    if (byIssuer.length === 1) return byIssuer[0].id
  }
  return null
}

export default function Entry() {
  const { ref, error } = useRefData()
  if (error) return <ErrorBox error={error} />
  if (!ref) return <Loading />
  return <EntryForm refMaps={ref} />
}

function EntryForm({ refMaps }: { refMaps: RefMaps }) {
  const toast = useToast()
  const { run, pending } = useMutate()
  const [f, setF] = useState<FormState>(() => emptyForm(refMaps))
  const [sms, setSms] = useState('')
  const [parsed, setParsed] = useState<ParsedMessage | null>(null)
  const set = (patch: Partial<FormState>) => setF((prev) => ({ ...prev, ...patch }))

  const pm = f.paymentMethodId ? refMaps.paymentMethodById.get(f.paymentMethodId) : undefined
  const categories = refMaps.categoriesOf(f.ledgerId)

  const pick = (s: Suggestion) => {
    const samePm = !f.paymentMethodId || f.paymentMethodId === s.paymentMethodId
    set({
      description: s.description,
      ledgerId: s.ledgerId,
      categoryId: s.categoryId,
      paymentMethodId: f.paymentMethodId ?? s.paymentMethodId,
      member: f.member || s.member || '',
      benefitIds: samePm ? s.benefitIds : f.benefitIds,
      amount: f.amount || (s.lastAmount > 0 ? String(s.lastAmount) : ''),
    })
  }

  const submit = async (e?: FormEvent) => {
    e?.preventDefault()
    const amount = parseAmount(f.amount)
    if (amount == null || amount === 0) {
      toast('금액을 입력하세요', 'error')
      return
    }
    if (!f.description.trim()) {
      toast('내용을 입력하세요', 'error')
      return
    }
    const created = await run(CREATE_TX, {
        input: {
          ledgerId: f.ledgerId,
          date: f.date,
          amount: f.refund ? -Math.abs(amount) : amount,
          categoryId: f.categoryId,
          paymentMethodId: f.paymentMethodId,
          description: f.description.trim(),
          memo: f.memo || null,
          member: f.member || null,
          status: f.status,
          installmentMonths: f.installmentMonths,
          excludeFromPerformance: f.excludeFromPerformance,
          benefitIds: f.benefitIds,
        },
      })
    if (!created) return
    toast(`${f.description} ${won(amount)}원 ${f.status === 'PLANNED' ? '예정으로 ' : ''}저장했습니다`)
    setF(emptyForm(refMaps, { date: f.date, ledgerId: f.ledgerId, paymentMethodId: f.paymentMethodId, status: f.status }))
  }

  const parseSms = async () => {
    if (!sms.trim()) return
    try {
      const { parseMessage: p } = await gql<{ parseMessage: ParsedMessage }>(PARSE_MESSAGE_QUERY, { text: sms })
      setParsed(p)
      set({
        amount: p.amount ? String(p.amount) : f.amount,
        refund: p.cancel,
        description: p.merchant ?? f.description,
        date: p.date ?? f.date,
        installmentMonths: p.installmentMonths ?? 1,
        paymentMethodId: matchCard(refMaps, p) ?? f.paymentMethodId,
      })
    } catch (err) {
      toast(String(err), 'error')
    }
  }

  const ingestSms = async () => {
    if (!sms.trim()) return
    const res = await run<{ ingestTransaction: { created: boolean; duplicate: boolean; matchedPlanned: boolean; warnings: string[]; transaction: Transaction } }>(
      INGEST,
      { input: { text: sms } },
    )
    if (!res) return
    const r = res.ingestTransaction
    const what = r.duplicate ? '이미 들어온 문자입니다' : r.matchedPlanned ? '예정 항목을 확정했습니다' : '저장했습니다'
    toast([`${r.transaction.description} ${won(r.transaction.amount)}원 — ${what}`, ...r.warnings].join('\n'))
    setSms('')
    setParsed(null)
  }

  return (
    <div className="stack" style={{ maxWidth: 980 }}>
      <div className="page-head">
        <h1>결제 입력</h1>
        <span className="text-2 small">입력한 내역은 가계부·카드별 내역·대시보드에 바로 반영됩니다</span>
      </div>
      <div className="grid-2" style={{ alignItems: 'start' }}>
        <Panel title="새 결제">
          <form className="stack" style={{ gap: 14 }} onSubmit={submit}>
            <div className="row" style={{ alignItems: 'flex-end', gap: 12 }}>
              <label className="field" style={{ flex: 1 }}>
                금액
                <input
                  className="input num"
                  style={{ height: 48, fontSize: 24, fontWeight: 650 }}
                  inputMode="numeric"
                  placeholder="0"
                  autoFocus
                  value={f.amount}
                  onChange={(e) => set({ amount: e.target.value })}
                />
              </label>
              <label className="check" style={{ paddingBottom: 12 }}>
                <input type="checkbox" checked={f.refund} onChange={(e) => set({ refund: e.target.checked })} />
                환불/취소 (−)
              </label>
            </div>
            <label className="field">
              내용
              <DescriptionInput value={f.description} onChange={(v) => set({ description: v })} onPick={pick} />
              <span className="hint">과거에 쓴 가맹점을 고르면 항목·결제수단이 자동으로 채워집니다</span>
            </label>
            <div className="form-grid">
              <label className="field">
                날짜
                <input className="input" type="date" value={f.date} onChange={(e) => set({ date: e.target.value })} />
              </label>
              <div className="field" style={{ justifyContent: 'flex-end' }}>
                <div className="row">
                  <button type="button" className="btn sm" onClick={() => set({ date: localToday() })}>
                    오늘
                  </button>
                  <button type="button" className="btn sm" onClick={() => set({ date: addDays(localToday(), -1) })}>
                    어제
                  </button>
                </div>
              </div>
            </div>
            <div className="field">
              장부
              <div className="segmented" style={{ alignSelf: 'flex-start' }}>
                {refMaps.ledgers.map((l) => (
                  <button
                    type="button"
                    key={l.id}
                    className={f.ledgerId === l.id ? 'on' : ''}
                    onClick={() => set({ ledgerId: l.id, categoryId: f.ledgerId === l.id ? f.categoryId : null })}
                  >
                    {l.name}
                  </button>
                ))}
              </div>
            </div>
            <div className="field">
              항목
              <div className="row wrap" style={{ gap: 6 }}>
                {categories.map((c) => (
                  <button
                    type="button"
                    key={c.id}
                    className={`btn sm ${f.categoryId === c.id ? 'primary' : ''}`}
                    onClick={() => set({ categoryId: f.categoryId === c.id ? null : c.id })}
                  >
                    {c.name}
                  </button>
                ))}
              </div>
            </div>
            <div className="form-grid">
              <label className="field">
                결제수단
                <select
                  className="input"
                  value={f.paymentMethodId ?? ''}
                  onChange={(e) => set({ paymentMethodId: e.target.value ? Number(e.target.value) : null, benefitIds: [] })}
                >
                  <option value="">선택 안 함</option>
                  {refMaps.activePaymentMethods.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </label>
              {pm?.kind === 'CREDIT_CARD' && (
                <label className="field">
                  할부
                  <select className="input" value={f.installmentMonths} onChange={(e) => set({ installmentMonths: Number(e.target.value) })}>
                    <option value={1}>일시불</option>
                    {[2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 18, 24].map((m) => (
                      <option key={m} value={m}>
                        {m}개월
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <label className="field">
                구성원
                <input className="input" list="entry-members" value={f.member} onChange={(e) => set({ member: e.target.value })} placeholder="예: 송화" />
                <datalist id="entry-members">
                  {refMaps.members.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
              </label>
              <label className="field">
                상태
                <select className="input" value={f.status} onChange={(e) => set({ status: e.target.value as TxStatus })}>
                  <option value="CONFIRMED">{STATUS_LABEL.CONFIRMED} (결제함)</option>
                  <option value="PLANNED">{STATUS_LABEL.PLANNED} (결제할 예정)</option>
                </select>
              </label>
            </div>
            {pm && pm.benefits.length > 0 && (
              <div className="field">
                {pm.name} 혜택
                <div className="row wrap" style={{ gap: 10 }}>
                  {pm.benefits.map((b) => (
                    <label key={b.id} className="check">
                      <input
                        type="checkbox"
                        checked={f.benefitIds.includes(b.id)}
                        onChange={(e) => set({ benefitIds: e.target.checked ? [...f.benefitIds, b.id] : f.benefitIds.filter((x) => x !== b.id) })}
                      />
                      {b.name}
                    </label>
                  ))}
                  <label className="check">
                    <input type="checkbox" checked={f.excludeFromPerformance} onChange={(e) => set({ excludeFromPerformance: e.target.checked })} />
                    전월실적 제외
                  </label>
                </div>
              </div>
            )}
            <label className="field">
              메모
              <input className="input" value={f.memo} onChange={(e) => set({ memo: e.target.value })} />
            </label>
            <div className="row">
              <button className="btn primary" style={{ height: 40, minWidth: 140 }} disabled={pending}>
                저장
              </button>
              <button type="button" className="btn ghost" onClick={() => setF(emptyForm(refMaps))}>
                초기화
              </button>
            </div>
          </form>
        </Panel>
        <div className="stack">
          <Panel title="카드 승인 문자로 입력" sub="문자/알림 내용을 붙여넣으세요">
            <textarea
              className="input"
              rows={5}
              style={{ width: '100%' }}
              placeholder={'[Web발신]\n신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원'}
              value={sms}
              onChange={(e) => setSms(e.target.value)}
            />
            <div className="row" style={{ marginTop: 8 }}>
              <button type="button" className="btn" onClick={parseSms} disabled={!sms.trim()}>
                해석해서 왼쪽에 채우기
              </button>
              <button type="button" className="btn primary" onClick={ingestSms} disabled={!sms.trim() || pending}>
                자동 분류로 바로 저장
              </button>
            </div>
            {parsed && (
              <dl className="kv" style={{ marginTop: 10 }}>
                <dt>금액</dt>
                <dd>{parsed.amount != null ? `${won(parsed.amount)}원${parsed.cancel ? ' (취소)' : ''}` : '-'}</dd>
                <dt>가맹점</dt>
                <dd>{parsed.merchant ?? '-'}</dd>
                <dt>일시</dt>
                <dd>{[parsed.date && shortDate(parsed.date), parsed.time].filter(Boolean).join(' ') || '-'}</dd>
                <dt>카드</dt>
                <dd>{[parsed.issuer, parsed.last4 && `끝자리 ${parsed.last4}`].filter(Boolean).join(' · ') || '-'}</dd>
              </dl>
            )}
            <p className="small muted" style={{ marginBottom: 0 }}>
              휴대폰에서 자동으로 보내려면 <Link to="/settings">설정 → 자동 입력 API</Link>를 참고하세요.
            </p>
          </Panel>
          <RecentEntries refMaps={refMaps} />
        </div>
      </div>
    </div>
  )
}

function RecentEntries({ refMaps }: { refMaps: RefMaps }) {
  const today = localToday()
  const q = useGql<{ transactions: TransactionPage }>(['recent', today], TRANSACTIONS_QUERY, {
    filter: { to: today, statuses: ['CONFIRMED'] },
    sort: 'DATE_DESC',
    limit: 12,
  })
  const items = q.data?.transactions.items ?? []
  return (
    <Panel title="최근 내역" sub="오늘까지 확정된 결제 (최신순)" flush>
      {q.error ? (
        <ErrorBox error={q.error} />
      ) : items.length === 0 ? (
        <div className="empty">최근 내역이 없습니다</div>
      ) : (
        <table className="table">
          <tbody>
            {items.map((t) => (
              <tr key={t.id}>
                <td className="nowrap">{shortDate(t.date)}</td>
                <td>
                  {t.description}
                  <div className="small muted">
                    {[refMaps.categoryById.get(t.categoryId ?? -1)?.name, refMaps.paymentMethodById.get(t.paymentMethodId ?? -1)?.name]
                      .filter(Boolean)
                      .join(' · ')}
                  </div>
                </td>
                <td className="num">
                  {won(t.amount)}
                  {t.status === 'PLANNED' && (
                    <div>
                      <span className="badge planned">예정</span>
                    </div>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  )
}
