import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react'
import { createPortal } from 'react-dom'

import { useGql } from '../api/hooks'
import { SUGGESTIONS_QUERY } from '../api/queries'
import type { Suggestion } from '../api/types'
import { won } from '../lib/format'

/**
 * 내용(가맹점) 입력 + 과거 내역 자동완성.
 * 항목을 고르면 onPick으로 그때의 장부/카테고리/결제수단을 넘겨 빈 칸을 채울 수 있게 한다.
 */
export default function DescriptionInput({
  value,
  onChange,
  onPick,
  ledgerId,
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

  const q = useGql<{ descriptionSuggestions: Suggestion[] }>(
    ['suggest', debounced, ledgerId ?? null],
    SUGGESTIONS_QUERY,
    { query: debounced, ledgerId: ledgerId ?? null },
    { enabled: open && debounced.length > 0, staleTime: 60_000 },
  )
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
              key={s.description}
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
                gap: 8,
              }}
            >
              <span style={{ flex: 1 }}>{s.description}</span>
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
