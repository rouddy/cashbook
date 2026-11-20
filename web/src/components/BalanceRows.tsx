import type { RefMaps } from '../api/hooks'
import type { MonthlySummary } from '../api/types'
import { won } from '../lib/format'

/** 이름 뒤 조사 '으로/로': 받침이 없거나 ㄹ 받침이면 '로' (용돈으로, 여행으로, 경조사비로) */
function withRo(name: string): string {
  const code = name.charCodeAt(name.length - 1) - 0xac00
  if (code < 0 || code > 11171) return `${name}(으)로`
  const final = code % 28
  return final === 0 || final === 8 ? `${name}로` : `${name}으로`
}

/**
 * 월잔액 계산 표의 행들: 수입 + 받은 이체 − (소비) − 보낸 이체 − 배분 = 월잔액.
 * 금액은 확정 기준이고 예정은 작게 덧붙인다. 장부를 정한 요약(`s.plan`이 있을 때)에서만 쓴다.
 */
export default function BalanceRows({ s, refMaps, showSpending = false }: { s: MonthlySummary; refMaps: RefMaps; showSpending?: boolean }) {
  const ledgerName = (id: number) => refMaps.ledgerById.get(id)?.name ?? '다른 장부'
  const planned = (v: number) => v !== 0 && <div className="small muted">+예정 {won(v)}</div>
  return (
    <>
      {(s.income !== 0 || s.plannedIncome !== 0) && (
        <tr>
          <td className="text-2">수입 (정기·기타수익)</td>
          <td className="num text-2">
            {won(s.income)}
            {planned(s.plannedIncome)}
          </td>
        </tr>
      )}
      {s.transfers
        .filter((t) => t.incoming)
        .map((t) => (
          <tr key={`in:${t.ledgerId}`}>
            <td className="text-2">+ {ledgerName(t.ledgerId)}에서 받은 이체</td>
            <td className="num text-2">
              {won(t.confirmed)}
              {planned(t.planned)}
            </td>
          </tr>
        ))}
      {showSpending && (
        <tr>
          <td className="text-2">− 소비</td>
          <td className="num text-2">
            {won(s.spending)}
            {planned(s.plannedSpending)}
          </td>
        </tr>
      )}
      {s.transfers
        .filter((t) => !t.incoming)
        .map((t) => (
          <tr key={`out:${t.ledgerId}`}>
            <td className="text-2">− {withRo(ledgerName(t.ledgerId))} 이체</td>
            <td className="num text-2">
              {won(t.confirmed)}
              {planned(t.planned)}
            </td>
          </tr>
        ))}
      {s.plan?.items
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
          {s.projectedBalance != null && s.projectedBalance !== s.balance && (
            <div className="small muted" style={{ fontWeight: 400 }}>
              예정 반영 {won(s.projectedBalance)}
            </div>
          )}
        </td>
      </tr>
    </>
  )
}
