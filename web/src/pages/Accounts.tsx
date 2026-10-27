import { useState, type FormEvent } from 'react'
import { Link } from 'react-router-dom'

import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import { ACCOUNT_HISTORY_QUERY, CREATE_ACCOUNT, DELETE_ACCOUNT, DELETE_BALANCE, FUNDS_QUERY, UPDATE_ACCOUNT, UPDATE_BALANCE } from '../api/queries'
import type { Account, AccountBalance, AccountKind, AvailableFunds } from '../api/types'
import { ErrorBox, Loading, Panel, StatTile } from '../components/Common'
import Modal from '../components/Modal'
import { useToast } from '../components/Toast'
import { ACCOUNT_KIND_LABEL, longDate, parseAmount, percentToRate, rateToPercent, shortDate, won } from '../lib/format'
import { localToday } from '../lib/month'

export default function Accounts() {
  const { ref, error } = useRefData()
  const [editing, setEditing] = useState<Account | 'new' | null>(null)
  const [history, setHistory] = useState<Account | null>(null)
  const [showArchived, setShowArchived] = useState(false)
  const funds = useGql<{ availableFunds: AvailableFunds }>(['funds'], FUNDS_QUERY)

  if (error) return <ErrorBox error={error} />
  if (!ref) return <Loading />
  const accounts = ref.accounts.filter((a) => showArchived || !a.archived).sort((a, b) => Number(a.archived) - Number(b.archived) || a.sortOrder - b.sortOrder)
  const included = ref.accounts.filter((a) => a.includeInAvailable && !a.archived)
  const f = funds.data?.availableFunds

  return (
    <div className="stack">
      <div className="page-head">
        <h1>은행 잔고</h1>
        <span className="spacer" />
        <label className="check">
          <input type="checkbox" checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} />
          보관된 계좌 보기
        </label>
        <button className="btn primary" onClick={() => setEditing('new')}>
          + 계좌 추가
        </button>
      </div>
      <div className="kpis">
        <StatTile label="여유자금 계산에 포함된 잔고" value={`${won(included.reduce((a, x) => a + x.balance, 0))}원`} delta={<span className="text-2">{included.length}개 계좌</span>} />
        <StatTile label="전체 잔고 (적금·투자 포함)" value={`${won(ref.accounts.filter((a) => !a.archived).reduce((a, x) => a + x.balance, 0))}원`} />
        {f && (
          <StatTile
            label="현재 여유자금"
            value={<span className={f.available < 0 ? 'bad' : ''}>{won(f.available)}원</span>}
            delta={
              <span className="text-2">
                카드 결제 예정 {won(f.cardDue)}원 차감 · <Link to="/">대시보드</Link>
              </span>
            }
          />
        )}
      </div>
      <Panel title="계좌" sub="잔고를 입력하면 기준일 이후 결제될 카드대금을 빼서 여유자금을 계산합니다" flush>
        {accounts.length === 0 ? (
          <div className="empty">
            계좌가 없습니다. <button className="btn sm" onClick={() => setEditing('new')}>계좌 추가</button>
          </div>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>계좌</th>
                  <th>종류</th>
                  <th className="num">잔고</th>
                  <th>기준일</th>
                  <th>잔고 입력</th>
                  <th>여유자금</th>
                  <th>연결 카드</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {accounts.map((a) => (
                  <AccountRow key={a.id} a={a} refMaps={ref} onEdit={() => setEditing(a)} onHistory={() => setHistory(a)} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
      {editing && <AccountModal account={editing === 'new' ? undefined : editing} onClose={() => setEditing(null)} />}
      {history && <HistoryModal account={history} onClose={() => setHistory(null)} />}
    </div>
  )
}

function AccountRow({ a, refMaps, onEdit, onHistory }: { a: Account; refMaps: RefMaps; onEdit: () => void; onHistory: () => void }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const [value, setValue] = useState('')
  const [date, setDate] = useState(localToday())
  const cards = refMaps.paymentMethods.filter((p) => p.accountId === a.id && !p.archived)
  const stale = a.balanceDate && a.balanceDate < localToday()

  const save = async (e: FormEvent) => {
    e.preventDefault()
    const v = parseAmount(value)
    if (v == null) {
      toast('잔고를 숫자로 입력하세요', 'error')
      return
    }
    if (!(await run(UPDATE_BALANCE, { id: a.id, balance: v, date }))) return
    setValue('')
    toast(`${a.name} 잔고를 ${won(v)}원으로 기록했습니다`)
  }

  const toggleInclude = () =>
    run(UPDATE_ACCOUNT, {
      id: a.id,
      input: {
        name: a.name,
        bank: a.bank,
        kind: a.kind,
        accountNumber: a.accountNumber,
        includeInAvailable: !a.includeInAvailable,
        interestRate: a.interestRate,
        maturityDate: a.maturityDate,
        memo: a.memo,
        archived: a.archived,
      },
    })

  return (
    <tr style={a.archived ? { opacity: 0.6 } : undefined}>
      <td>
        <b>{a.name}</b>
        <div className="small muted">{[a.bank, a.accountNumber].filter(Boolean).join(' · ')}</div>
      </td>
      <td className="text-2">{ACCOUNT_KIND_LABEL[a.kind]}</td>
      <td className="num" style={{ fontWeight: 650 }}>
        {won(a.balance)}
      </td>
      <td className="nowrap">
        {a.balanceDate ? (
          <span className={stale ? 'text-2' : ''} title={longDate(a.balanceDate)}>
            {shortDate(a.balanceDate)}
          </span>
        ) : (
          <span className="badge warn">미입력</span>
        )}
      </td>
      <td>
        <form className="row" onSubmit={save}>
          <input className="input num" inputMode="numeric" placeholder="현재 잔고" value={value} onChange={(e) => setValue(e.target.value)} style={{ width: 120 }} aria-label={`${a.name} 잔고`} />
          <input className="input" type="date" value={date} onChange={(e) => setDate(e.target.value)} style={{ width: 140 }} aria-label="기준일" />
          <button className="btn sm primary" disabled={pending || !value}>
            기록
          </button>
        </form>
      </td>
      <td>
        <label className="check">
          <input type="checkbox" checked={a.includeInAvailable} onChange={toggleInclude} disabled={pending} />
          포함
        </label>
      </td>
      <td className="small">
        {cards.length ? cards.map((c) => <Link key={c.id} to={`/cards/${c.id}`} style={{ marginRight: 6 }}>{c.name}</Link>) : <span className="muted">-</span>}
      </td>
      <td className="nowrap">
        <button className="btn sm ghost" onClick={onHistory}>
          이력
        </button>
        <button className="btn sm ghost" onClick={onEdit}>
          편집
        </button>
      </td>
    </tr>
  )
}

function AccountModal({ account, onClose }: { account?: Account; onClose: () => void }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const [name, setName] = useState(account?.name ?? '')
  const [bank, setBank] = useState(account?.bank ?? '')
  const [kind, setKind] = useState<AccountKind>(account?.kind ?? 'CHECKING')
  const [accountNumber, setAccountNumber] = useState(account?.accountNumber ?? '')
  const initialBalance = account ? String(account.balance) : ''
  // 새 계좌는 오늘 기준, 기준일이 없던 계좌는 비워 둔다 (편집만 했는데 기준일이 오늘로 찍히지 않게)
  const initialDate = account ? (account.balanceDate ?? '') : localToday()
  const [balance, setBalance] = useState(initialBalance)
  const [balanceDate, setBalanceDate] = useState(initialDate)
  const [include, setInclude] = useState(account?.includeInAvailable ?? true)
  const [interestRate, setInterestRate] = useState(account?.interestRate != null ? rateToPercent(account.interestRate) : '')
  const [maturityDate, setMaturityDate] = useState(account?.maturityDate ?? '')
  const [memo, setMemo] = useState(account?.memo ?? '')
  const [archived, setArchived] = useState(account?.archived ?? false)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    const bal = balance.trim() === '' ? null : parseAmount(balance)
    if (balance.trim() !== '' && bal == null) {
      toast('잔고를 숫자로 입력하세요', 'error')
      return
    }
    // 편집 때는 잔고나 기준일을 실제로 바꿨을 때만 잔고 기록을 남긴다
    const balanceTouched = !account || balance !== initialBalance || balanceDate !== initialDate
    const input = {
      name: name.trim(),
      bank: bank || null,
      kind,
      accountNumber: accountNumber || null,
      balance: balanceTouched ? bal : null,
      balanceDate: balanceTouched && bal != null && balanceDate ? balanceDate : null,
      includeInAvailable: include,
      interestRate: interestRate ? percentToRate(interestRate) : null,
      maturityDate: maturityDate || null,
      memo: memo || null,
      archived,
    }
    const saved = account ? await run(UPDATE_ACCOUNT, { id: account.id, input }) : await run(CREATE_ACCOUNT, { input })
    if (saved) onClose()
  }

  const remove = async () => {
    if (!account || !window.confirm(`'${account.name}' 계좌와 잔고 이력을 삭제할까요? (연결된 카드의 결제 계좌는 비워집니다)`)) return
    if (await run(DELETE_ACCOUNT, { id: account.id })) onClose()
  }

  return (
    <Modal
      title={account ? '계좌 편집' : '계좌 추가'}
      onClose={onClose}
      footer={
        <>
          {account && (
            <button className="btn danger" style={{ marginRight: 'auto' }} onClick={remove} disabled={pending}>
              삭제
            </button>
          )}
          <button className="btn" onClick={onClose}>
            취소
          </button>
          <button className="btn primary" form="account-form" disabled={pending || !name.trim()}>
            저장
          </button>
        </>
      }
    >
      <form id="account-form" className="form-grid" onSubmit={submit}>
        <label className="field">
          이름 *
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="예: 생활비 통장" autoFocus />
        </label>
        <label className="field">
          은행
          <input className="input" value={bank} onChange={(e) => setBank(e.target.value)} />
        </label>
        <label className="field">
          종류
          <select className="input" value={kind} onChange={(e) => setKind(e.target.value as AccountKind)}>
            {Object.entries(ACCOUNT_KIND_LABEL).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          계좌번호 (선택)
          <input className="input" value={accountNumber} onChange={(e) => setAccountNumber(e.target.value)} />
        </label>
        <label className="field">
          잔고
          <input className="input num" inputMode="numeric" value={balance} onChange={(e) => setBalance(e.target.value)} />
        </label>
        <label className="field">
          잔고 기준일
          <input className="input" type="date" value={balanceDate} onChange={(e) => setBalanceDate(e.target.value)} />
        </label>
        <label className="field">
          금리 (%)
          <input className="input num" inputMode="decimal" value={interestRate} onChange={(e) => setInterestRate(e.target.value)} />
        </label>
        <label className="field">
          만기일
          <input className="input" type="date" value={maturityDate} onChange={(e) => setMaturityDate(e.target.value)} />
        </label>
        <label className="field" style={{ gridColumn: '1 / -1' }}>
          메모
          <input className="input" value={memo} onChange={(e) => setMemo(e.target.value)} />
        </label>
        <label className="check">
          <input type="checkbox" checked={include} onChange={(e) => setInclude(e.target.checked)} />
          여유자금 계산에 포함 (생활비·카드대금 출금 계좌)
        </label>
        {account && (
          <label className="check">
            <input type="checkbox" checked={archived} onChange={(e) => setArchived(e.target.checked)} />
            보관 (목록에서 숨김)
          </label>
        )}
      </form>
    </Modal>
  )
}

function HistoryModal({ account, onClose }: { account: Account; onClose: () => void }) {
  const q = useGql<{ account: { id: number; history: AccountBalance[] } | null }>(['accountHistory', account.id], ACCOUNT_HISTORY_QUERY, { id: account.id })
  const { run } = useMutate()
  const rows = q.data?.account?.history ?? []
  return (
    <Modal title={`${account.name} 잔고 이력`} onClose={onClose}>
      {q.isLoading ? (
        <Loading />
      ) : rows.length === 0 ? (
        <div className="empty">기록이 없습니다</div>
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>기준일</th>
              <th className="num">잔고</th>
              <th className="num">변화</th>
              <th>메모</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((h, i) => {
              const prev = rows[i + 1]
              const diff = prev ? h.balance - prev.balance : null
              return (
                <tr key={h.id}>
                  <td>{longDate(h.balanceDate)}</td>
                  <td className="num">{won(h.balance)}</td>
                  <td className={`num ${diff == null ? '' : diff < 0 ? 'bad' : 'good'}`}>{diff == null ? '-' : `${diff > 0 ? '+' : ''}${won(diff)}`}</td>
                  <td className="text-2">{h.memo ?? ''}</td>
                  <td>
                    <button className="btn sm ghost danger" onClick={() => window.confirm('이 기록을 삭제할까요? (현재 잔고는 바뀌지 않습니다)') && run(DELETE_BALANCE, { id: h.id })}>
                      ✕
                    </button>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
    </Modal>
  )
}
