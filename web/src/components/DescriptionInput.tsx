import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react'
import { createPortal } from 'react-dom'

import { type RefMaps, useGql } from '../api/hooks'
import { SUGGESTIONS_QUERY } from '../api/queries'
import type { Suggestion } from '../api/types'
import { won } from '../lib/format'
import { localToday, monthsBefore } from '../lib/month'

/** 자동완성으로 찾는 범위 (없으면 전체 내역에서 내용마다 가장 많이 쓴 분류 하나) */
export interface SuggestScope {
  /** 기준일(오늘보다 늦으면 오늘)에서 이만큼 전부터 쓴 내역만 찾는다 */
  recentMonths?: number
  /** 같은 내용이라도 장부·항목이 다르면 항목을 붙여 따로 보여 준다 */
  byCategory?: boolean
}

/**
 * 내용(가맹점) 입력 + 과거 내역 자동완성.
 * 항목을 고르면 onPick으로 그때의 장부/카테고리/결제수단을 넘겨 빈 칸을 채울 수 있게 한다.
 */
export default function DescriptionInput({
  value,
  onChange,
  onPick,
  ledgerId,
  scope,
  baseDate,
  refMaps,
  className = 'input',
  placeholder = '내용 (가맹점)',
  autoFocus,
  onKeyDown,
  onBlur,
  ariaLabel = '내용',
}: {
  value: string
  onChange: (v: string) => void
  onPick?: (s: Suggestion) => void
  ledgerId?: number | null
  scope?: SuggestScope
  /** `scope.recentMonths`의 기준일 (입력 중인 거래의 날짜, 없으면 오늘) */
  baseDate?: string
  /** `scope.byCategory`일 때 항목·장부 이름 */
  refMaps?: RefMaps
  className?: string
  placeholder?: string
  autoFocus?: boolean
  onKeyDown?: (e: KeyboardEvent<HTMLInputElement>) => void
  onBlur?: () => void
  ariaLabel?: string
}) {
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(-1)
  const [debounced, setDebounced] = useState(value)
  const listId = useId()
  const inputRef = useRef<HTMLInputElement>(null)
  const [listStyle, setListStyle] = useState<CSSProperties | null>(null)

  useEffect(() => {
    const t = window.setTimeout(() => setDebounced(value.trim()), 200)
    return () => window.clearTimeout(t)
  }, [value])

  const since = scope?.recentMonths ? monthsBefore(minDate(baseDate, localToday()), scope.recentMonths) : null
  const byCategory = scope?.byCategory ?? false
  const q = useGql<{ descriptionSuggestions: Suggestion[] }>(
    ['suggest', debounced, ledgerId ?? null, since, byCategory],
    SUGGESTIONS_QUERY,
    { query: debounced, ledgerId: ledgerId ?? null, since, byCategory },
    { enabled: open && debounced.length > 0, staleTime: 60_000 },
  )
  // 장부를 정해 두지 않았으면 다른 장부의 같은 이름 항목과 구별되게 장부도 붙인다
  const showLedger = ledgerId == null && (refMaps?.ledgers.length ?? 0) > 1
  const classification = (s: Suggestion) => {
    const category = s.categoryId != null ? refMaps?.categoryById.get(s.categoryId)?.name : undefined
    const ledger = showLedger ? refMaps?.ledgerById.get(s.ledgerId)?.name : undefined
    return [ledger, category ?? '항목 없음'].filter(Boolean).join(' · ')
  }
  const items = open && debounced.length > 0 ? (q.data?.descriptionSuggestions ?? []) : []
  const hasItems = items.length > 0

  // 목록을 body에 고정 위치로 띄운다 (표의 스크롤 영역에 잘리지 않게). 아래 공간이 좁으면 위로 연다.
  useLayoutEffect(() => {
    if (!hasItems) return
    const update = () => {
      const r = inputRef.current?.getBoundingClientRect()
      if (!r) return
      const below = window.innerHeight - r.bottom - 8
      const above = r.top - 8
      const openUp = below < 180 && above > below
      setListStyle({
        position: 'fixed',
        left: r.left,
        minWidth: Math.max(260, r.width),
        maxHeight: Math.min(280, openUp ? above : below),
        ...(openUp ? { bottom: window.innerHeight - r.top + 2 } : { top: r.bottom + 2 }),
      })
    }
    update()
    window.addEventListener('scroll', update, true)
    window.addEventListener('resize', update)
    return () => {
      window.removeEventListener('scroll', update, true)
      window.removeEventListener('resize', update)
    }
  }, [hasItems])

  const pick = (s: Suggestion) => {
    onChange(s.description)
    onPick?.(s)
    setOpen(false)
    setActive(-1)
  }

  const keyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (items.length > 0 && open) {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setActive((a) => Math.min(items.length - 1, a + 1))
        return
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActive((a) => Math.max(-1, a - 1))
        return
      }
      if (e.key === 'Enter' && active >= 0) {
        e.preventDefault()
        pick(items[active])
        return
      }
      if (e.key === 'Escape') {
        e.stopPropagation()
        setOpen(false)
        return
      }
    }
    onKeyDown?.(e)
  }

  return (
    <div style={{ position: 'relative', width: '100%' }}>
      <input
        ref={inputRef}
        className={className}
        value={value}
        placeholder={placeholder}
        autoFocus={autoFocus}
        aria-label={ariaLabel}
        role="combobox"
        aria-expanded={items.length > 0}
        aria-controls={listId}
        aria-autocomplete="list"
        onChange={(e) => {
          onChange(e.target.value)
          setOpen(true)
          setActive(-1)
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => {
          // 목록 클릭이 먼저 처리되도록 살짝 늦게 닫는다
          window.setTimeout(() => {
            setOpen(false)
            onBlur?.()
          }, 120)
        }}
        onKeyDown={keyDown}
        style={{ width: '100%' }}
      />
      {hasItems &&
        listStyle &&
        createPortal(
        <ul
          id={listId}
          role="listbox"
          className="panel"
          style={{
            ...listStyle,
            zIndex: 70,
            margin: 0,
            padding: 4,
            listStyle: 'none',
            overflowY: 'auto',
          }}
        >
          {items.map((s, i) => (
            <li
              key={JSON.stringify([s.description, s.ledgerId, s.categoryId])}
              role="option"
              aria-selected={i === active}
              onMouseDown={(e) => {
                e.preventDefault()
                pick(s)
              }}
              onMouseEnter={() => setActive(i)}
              style={{
                padding: '6px 8px',
                borderRadius: 6,
                cursor: 'pointer',
                background: i === active ? 'var(--accent-soft)' : undefined,
                display: 'flex',
                alignItems: 'center',
                gap: 8,
              }}
            >
              <span style={{ flex: 1 }}>{s.description}</span>
              {byCategory && (
                <span className="badge" style={{ flexShrink: 0 }}>
                  {classification(s)}
                </span>
              )}
              <span className="small muted">
                {s.count}회 · 최근 {won(s.lastAmount)}
              </span>
            </li>
          ))}
        </ul>,
          document.body,
        )}
    </div>
  )
}

/** 둘 중 이른 날짜 (YYYY-MM-DD 문자열 비교) */
function minDate(a: string | undefined, b: string): string {
  return a && a < b ? a : b
}
