import { useState } from 'react'

import { type RefMaps, useMutate } from '../api/hooks'
import { DELETE_TXS } from '../api/queries'
import type { Transaction } from '../api/types'
import { shortDate, won } from '../lib/format'
import { localToday } from '../lib/month'
import type { useTxSelection } from '../lib/selection'
import { isSplit, splitTotal } from '../lib/split'
import { Amount } from './Common'
import Modal from './Modal'
import { useToast } from './Toast'

/** 분할한 결제는 어느 쪽을 골라도 결제 하나 (원래 거래 id로 묶는다) */
const paymentKey = (t: Transaction) => (isSplit(t) ? (t.splitParentId ?? t.id) : t.id)

/**
 * 선택 모드의 `삭제` 버튼. 바로 지우지 않고 확인 창에서 지울 거래를 보여준 뒤 지운다.
 * 규칙은 서버 `deleteTransactions`와 같다 (분할 결제는 전체, 반복 지출의 이번 달 이후 항목은 건너뜀).
 */
export default function DeleteSelectedButton({ chosen, refMaps, onDone }: { chosen: Transaction[]; refMaps: RefMaps; onDone: () => void }) {
  const [confirming, setConfirming] = useState(false)
  return (
    <>
      <button
        className="btn sm danger-solid"
        disabled={chosen.length === 0}
        title={chosen.length === 0 ? '삭제할 거래를 체크하세요' : undefined}
        onClick={() => setConfirming(true)}
      >
        삭제
      </button>
      {confirming && (
        <DeleteConfirm
          chosen={chosen}
          refMaps={refMaps}
          onClose={() => setConfirming(false)}
          onDone={() => {
            setConfirming(false)
            onDone()
          }}
        />
      )}
    </>
  )
}

/** 패널 머리글용 선택 모드 버튼: `선택` → 체크박스와 `삭제` (장부 이동이 없는 카드 화면) */
export function SelectionActions({ sel, refMaps }: { sel: ReturnType<typeof useTxSelection>; refMaps: RefMaps }) {
  return (
    <div className="row">
      {sel.selecting && <span className="small text-2">{sel.chosen.length}건 선택</span>}
      {sel.selecting && <DeleteSelectedButton chosen={sel.chosen} refMaps={refMaps} onDone={sel.stop} />}
      <button className={`btn sm ${sel.selecting ? 'ghost' : ''}`} aria-pressed={sel.selecting} onClick={sel.selecting ? sel.stop : sel.start}>
        {sel.selecting ? '선택 취소' : '선택'}
      </button>
    </div>
  )
}

function DeleteConfirm({ chosen, refMaps, onClose, onDone }: { chosen: Transaction[]; refMaps: RefMaps; onClose: () => void; onDone: () => void }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  // 분할 묶음의 여러 쪽을 골랐어도(전체 장부 보기) 결제 하나로 보여준다
  const payments = [...new Map(chosen.map((t) => [paymentKey(t), t])).values()]
  // 반복 규칙이 이번 달 이후에 만든 항목은 지우지 않고 건너뜀으로 남는다 (서버와 같은 규칙)
  const thisMonth = localToday().slice(0, 7)
  const keepsSlot = payments.some((t) => t.recurringRuleId != null && t.recurringMonth != null && t.recurringMonth >= thisMonth)
  const ledgerName = (id: number) => refMaps.ledgerById.get(id)?.name ?? ''

  const remove = async () => {
    const res = await run<{ deleteTransactions: { deleted: number; skipped: number } }>(DELETE_TXS, { ids: chosen.map((t) => t.id) })
    if (!res) return
    const { deleted, skipped } = res.deleteTransactions
    toast(`${deleted + skipped}건을 삭제했습니다${skipped ? ` (반복 지출 ${skipped}건은 '건너뜀'으로 남김)` : ''}`)
    onDone()
  }

  return (
    <Modal
      title="거래 삭제"
      onClose={onClose}
      width={480}
      footer={
        <>
          {/* Enter를 눌러도 지워지지 않도록 취소에 포커스 */}
          <button className="btn" autoFocus onClick={onClose}>
            취소
          </button>
          <button className="btn danger-solid" disabled={pending} onClick={remove}>
            {payments.length}건 삭제
          </button>
        </>
      }
    >
      <div className="stack">
        <div>
          선택한 거래 <b>{payments.length}건</b>을 삭제할까요? 삭제한 거래는 되돌릴 수 없습니다.
        </div>
        <div className="table-wrap" style={{ maxHeight: 280, overflowY: 'auto', border: '1px solid var(--border)', borderRadius: 8 }}>
          <table className="table">
            <tbody>
              {payments.map((t) => (
                <tr key={t.id}>
                  <td className="text-2" style={{ width: 84, whiteSpace: 'nowrap' }}>
                    {shortDate(t.date)}
                  </td>
                  <td>
                    {t.description}
                    {isSplit(t) && (
                      <div className="small text-2">
                        나눈 결제 전체 ({[t, ...t.splitWith].map((p) => `${ledgerName(p.ledgerId)} ${won(p.amount)}`).join(' + ')})
                      </div>
                    )}
                  </td>
                  <td className="num">
                    <Amount value={isSplit(t) ? splitTotal(t) : t.amount} income={t.isIncome} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {payments.some(isSplit) && <div className="small text-2">여러 장부에 나눈 결제는 다른 장부에 나눈 몫까지 함께 삭제됩니다.</div>}
        {keepsSlot && <div className="small text-2">반복 지출이 만든 이번 달 이후 항목은 다시 생기지 않도록 '건너뜀'으로 남습니다.</div>}
      </div>
    </Modal>
  )
}
