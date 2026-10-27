import { useMutation, useQuery, useQueryClient, type UseQueryOptions } from '@tanstack/react-query'
import { useCallback, useMemo } from 'react'

import { errorMessage, gql } from './client'
import { REF_QUERY } from './queries'
import type { Account, AppInfo, Category, Ledger, PaymentMethod } from './types'
import { useToast } from '../components/Toast'

export interface RefData {
  appInfo: AppInfo
  ledgers: Ledger[]
  categories: Category[]
  paymentMethods: PaymentMethod[]
  accounts: Account[]
  members: string[]
}

export interface RefMaps extends RefData {
  categoryById: Map<number, Category>
  paymentMethodById: Map<number, PaymentMethod>
  ledgerById: Map<number, Ledger>
  accountById: Map<number, Account>
  defaultLedger: Ledger | undefined
  /** 장부별 사용 중인 카테고리 (정렬됨) */
  categoriesOf: (ledgerId: number | null | undefined) => Category[]
  /** 보관하지 않은 결제수단 (정렬됨) */
  activePaymentMethods: PaymentMethod[]
}

/** 장부/카테고리/결제수단/계좌 등 거의 바뀌지 않는 기준 데이터 */
export function useRefData() {
  const q = useQuery({
    queryKey: ['ref'],
    queryFn: () => gql<RefData>(REF_QUERY),
    staleTime: 60_000,
  })
  const maps = useMemo<RefMaps | undefined>(() => {
    const d = q.data
    if (!d) return undefined
    const categoryById = new Map(d.categories.map((c) => [c.id, c]))
    const paymentMethodById = new Map(d.paymentMethods.map((p) => [p.id, p]))
    const ledgerById = new Map(d.ledgers.map((l) => [l.id, l]))
    const accountById = new Map(d.accounts.map((a) => [a.id, a]))
    const sorted = [...d.categories].sort((a, b) => a.ledgerId - b.ledgerId || a.sortOrder - b.sortOrder || a.id - b.id)
    return {
      ...d,
      categoryById,
      paymentMethodById,
      ledgerById,
      accountById,
      defaultLedger: d.ledgers.find((l) => l.isDefault) ?? d.ledgers[0],
      categoriesOf: (ledgerId) => sorted.filter((c) => !c.archived && (ledgerId == null || c.ledgerId === ledgerId)),
      activePaymentMethods: d.paymentMethods.filter((p) => !p.archived),
    }
  }, [q.data])
  return { ...q, ref: maps }
}

export function useGql<T>(
  key: unknown[],
  query: string,
  variables?: Record<string, unknown>,
  options?: Omit<UseQueryOptions<T>, 'queryKey' | 'queryFn'>,
) {
  return useQuery<T>({
    queryKey: key,
    queryFn: () => gql<T>(query, variables),
    placeholderData: (prev) => prev,
    ...options,
  })
}

/**
 * 뮤테이션 실행 후 모든 조회를 다시 불러온다 (데이터가 작아서 범위를 좁히지 않는다).
 * 실패하면 토스트로 알리고 `undefined`를 돌려준다 (예외를 던지지 않는다).
 * 성공해야 이어지는 동작(모달 닫기 등)은 결과가 있는지 확인하고 한다.
 */
export function useMutate() {
  const qc = useQueryClient()
  const toast = useToast()
  const m = useMutation({
    mutationFn: ({ query, variables }: { query: string; variables?: Record<string, unknown> }) =>
      gql<Record<string, unknown>>(query, variables),
    onSuccess: () => qc.invalidateQueries(),
    onError: (e) => toast(errorMessage(e), 'error'),
  })
  const { mutateAsync } = m
  const run = useCallback(
    async <T = Record<string, unknown>>(query: string, variables?: Record<string, unknown>): Promise<T | undefined> => {
      try {
        return (await mutateAsync({ query, variables })) as T
      } catch {
        return undefined
      }
    },
    [mutateAsync],
  )
  return { run, pending: m.isPending }
}
