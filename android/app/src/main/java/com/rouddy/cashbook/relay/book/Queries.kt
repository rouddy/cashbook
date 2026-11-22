package com.rouddy.cashbook.relay.book

/**
 * 앱이 쓰는 가계부 GraphQL 문서 (`POST /api/graphql`). 웹의 `web/src/api/queries.ts`와 같은 스키마를 쓴다.
 * 서버 스키마에서 이 필드들을 바꾸면 [BookModels.kt]와 함께 고친다.
 */
object Queries {
    private const val TX_FIELDS = "id ledgerId date amount categoryId paymentMethodId description memo member status " +
        "installmentMonths source rawText isIncome transferLedgerId splitParentId benefitIds createdAt"

    const val REF = """query Ref {
  ledgers { id name isDefault sortOrder }
  categories(includeArchived: true) { id ledgerId name kind archived }
  paymentMethods(includeArchived: true) { id name kind isCard archived benefits { id name kind stackable } }
  members
}"""

    /** 월 요약 + 카드 실적 + 여유자금(지금 기준) + 예정 거래 */
    const val DASHBOARD = $$"""query Dashboard($month: YearMonth!, $ledgerId: Int, $from: NaiveDate!, $to: NaiveDate!) {
  monthlySummary(month: $month, ledgerId: $ledgerId, averageMonths: 6, historyMonths: 1) {
    month ledgerId averageMonthsUsed spending plannedSpending income plannedIncome transferIn plannedTransferIn
    transferOut plannedTransferOut averageSpending budgetTotal balance projectedBalance
    categories { categoryId ledgerId name kind confirmed planned count average budget }
    daily { day date spending cumulative projectedCumulative averageCumulative }
  }
  cardsMonthly(month: $month) {
    paymentMethodId spending plannedSpending performance projectedPerformance performanceSteps target remainingToTarget
    currentLimit limitUsed discountTotal pointTotal mileageTotal achievedTier { minSpending benefitLimit }
    nextTier { minSpending benefitLimit } remainingToNextTier
  }
  availableFunds {
    today accountsTotal balanceDate cardDue cardDuePlanned unreflectedOutflow plannedOutflow plannedInflow
    available availableAfterPlanned
    cards { paymentMethodId counted nextPayment { month periodStart periodEnd paymentDate confirmedAmount plannedAmount closed paid } }
  }
  planned: transactions(filter: { from: $from, to: $to, statuses: [PLANNED], ledgerId: $ledgerId }, sort: DATE_ASC, limit: 100) {
    totalCount items { $$TX_FIELDS }
  }
}"""

    const val TRANSACTIONS = $$"""query Recent($filter: TransactionFilter!, $offset: Int!, $limit: Int!) {
  transactions(filter: $filter, sort: DATE_DESC, offset: $offset, limit: $limit) { totalCount items { $$TX_FIELDS } }
}"""

    const val SUGGESTIONS = $$"""query Suggest($query: String!, $since: NaiveDate) {
  descriptionSuggestions(query: $query, limit: 6, since: $since) {
    description ledgerId categoryId paymentMethodId member benefitIds lastAmount lastDate count
  }
}"""

    const val CREATE_TX = $$"""mutation CreateTx($input: TransactionInput!) {
  createTransaction(input: $input) { id description amount status }
}"""

    const val CONFIRM_TX = $$"""mutation ConfirmTx($id: Int!) { confirmTransaction(id: $id) { id description amount status } }"""

    const val SKIP_TX = $$"""mutation SkipTx($id: Int!) { skipTransaction(id: $id) { id description amount status } }"""

    /** 연결 확인: 이 토큰으로 GraphQL을 쓸 수 있는지 (토큰 권한을 합치기 전 서버는 예전 자동 입력 전용 토큰에 403) */
    const val PROBE = "query Probe { appInfo { version } }"
}
