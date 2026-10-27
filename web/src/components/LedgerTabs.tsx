import type { Ledger } from '../api/types'

/** 장부 선택. value가 ''이면 전체 */
export default function LedgerTabs({
  ledgers,
  value,
  onChange,
  allowAll = true,
}: {
  ledgers: Ledger[]
  value: string
  onChange: (v: string) => void
  allowAll?: boolean
}) {
  return (
    <div className="segmented" role="tablist" aria-label="장부">
      {allowAll && (
        <button role="tab" aria-selected={value === ''} className={value === '' ? 'on' : ''} onClick={() => onChange('')}>
          전체
        </button>
      )}
      {ledgers.map((l) => (
        <button
          key={l.id}
          role="tab"
          aria-selected={value === String(l.id)}
          className={value === String(l.id) ? 'on' : ''}
          onClick={() => onChange(String(l.id))}
        >
          {l.name}
        </button>
      ))}
    </div>
  )
}
