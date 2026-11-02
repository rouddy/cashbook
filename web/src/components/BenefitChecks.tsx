import type { CardBenefit } from '../api/types'
import { benefitRateText, toggleBenefit } from '../lib/benefits'

/** 거래에 적용할 혜택 고르기. 중복 적용이 아닌 혜택은 하나만 고를 수 있다. */
export default function BenefitChecks({ benefits, selected, onChange }: { benefits: CardBenefit[]; selected: number[]; onChange: (ids: number[]) => void }) {
  return (
    <>
      {benefits.map((b) => (
        <label key={b.id} className="check" title={b.stackable ? '다른 혜택과 함께 받는 혜택' : undefined}>
          <input type="checkbox" checked={selected.includes(b.id)} onChange={(e) => onChange(toggleBenefit(selected, b, e.target.checked, benefits))} />
          {b.stackable && '+ '}
          {b.name} <span className="muted small">{benefitRateText(b)}</span>
        </label>
      ))}
    </>
  )
}
