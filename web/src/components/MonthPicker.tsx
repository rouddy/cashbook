import { addMonths, currentMonth, monthLabel } from '../lib/month'

export default function MonthPicker({ month, onChange }: { month: string; onChange: (m: string) => void }) {
  const isCurrent = month === currentMonth()
  return (
    <div className="row" role="group" aria-label="월 선택">
      <button className="btn icon" onClick={() => onChange(addMonths(month, -1))} aria-label="이전 달">
        ‹
      </button>
      <label className="row" style={{ gap: 0 }}>
        <span className="sr-only">월</span>
        <input
          className="input"
          type="month"
          value={month}
          onChange={(e) => e.target.value && onChange(e.target.value)}
          style={{ width: 150, fontWeight: 650 }}
          aria-label={monthLabel(month)}
        />
      </label>
      <button className="btn icon" onClick={() => onChange(addMonths(month, 1))} aria-label="다음 달">
        ›
      </button>
      {!isCurrent && (
        <button className="btn sm ghost" onClick={() => onChange(currentMonth())}>
          이번 달
        </button>
      )}
    </div>
  )
}
