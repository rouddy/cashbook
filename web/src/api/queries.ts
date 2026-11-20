// GraphQL 문서. 화면마다 필요한 필드만 가져온다.

export const BENEFIT_FIELDS = `id paymentMethodId name kind rate minAmount perTxMax monthlyCap sharesCardLimit excludesPerformance stackable sortOrder`

export const PAYMENT_METHOD_FIELDS = `id name kind issuer productName last4 aliases accountId paymentDay closingDay closingMonthOffset
  monthlyTarget annualFee expiresOn color memo sortOrder archived isCard billingConfigured usagePeriodLabel
  benefits { ${BENEFIT_FIELDS} } tiers { id paymentMethodId minSpending benefitLimit }`

export const CATEGORY_FIELDS = `id ledgerId name kind monthlyBudget isFixed color sortOrder archived`

export const ACCOUNT_FIELDS = `id name bank kind accountNumber balance balanceDate includeInAvailable interestRate maturityDate memo sortOrder archived updatedAt`

export const TX_FIELDS = `id ledgerId date amount categoryId paymentMethodId description memo member status installmentMonths
  excludeFromPerformance recurringRuleId recurringMonth source rawText benefitIds statementMonth paymentDate isIncome
  splitParentId splitWith { id ledgerId amount categoryId } transferLedgerId`

export const STATEMENT_FIELDS = `month periodStart periodEnd paymentDate confirmedAmount plannedAmount closed paid`

export const REF_QUERY = `query Ref {
  appInfo { version today timezone authEnabled planMonthsAhead }
  ledgers { id name description isDefault sortOrder }
  categories(includeArchived: true) { ${CATEGORY_FIELDS} }
  paymentMethods(includeArchived: true) { ${PAYMENT_METHOD_FIELDS} }
  accounts(includeArchived: true) { ${ACCOUNT_FIELDS} }
  members
}`

export const TRANSACTIONS_QUERY = `query Transactions($filter: TransactionFilter!, $sort: TransactionSort = DATE_ASC, $limit: Int = 1000) {
  transactions(filter: $filter, sort: $sort, limit: $limit) {
    totalCount confirmedTotal plannedTotal incomeTotal plannedIncomeTotal
    transferInTotal plannedTransferInTotal transferOutTotal plannedTransferOutTotal
    items { ${TX_FIELDS} }
  }
}`

export const SUMMARY_QUERY = `query Summary($month: YearMonth!, $ledgerId: Int, $averageMonths: Int = 6, $historyMonths: Int = 12) {
  monthlySummary(month: $month, ledgerId: $ledgerId, averageMonths: $averageMonths, historyMonths: $historyMonths) {
    month ledgerId averageMonths averageMonthsUsed spending plannedSpending fixedSpending variableSpending
    income plannedIncome transferIn plannedTransferIn transferOut plannedTransferOut averageSpending averageFixedSpending
    averageVariableSpending budgetTotal balance projectedBalance
    plan { allocations memberAllowances items { id ledgerId kind name amount member sortOrder } }
    categories { categoryId ledgerId name kind isFixed confirmed planned count average budget }
    paymentMethods { paymentMethodId name confirmed planned count }
    transfers { ledgerId incoming confirmed planned count }
    members { member spending planned allowance remaining }
    daily { day date spending cumulative projectedCumulative averageCumulative }
    history { month spending plannedSpending income fixedSpending }
  }
}`

export const FUNDS_QUERY = `query Funds {
  availableFunds {
    today accountsTotal balanceDate cardDue cardDuePlanned unreflectedOutflow plannedOutflow plannedInflow
    available availableAfterPlanned unconfiguredCardIds
    cards { paymentMethodId billingConfigured counted cutoffDate due duePlanned recentSpending
      nextPayment { ${STATEMENT_FIELDS} } statements { ${STATEMENT_FIELDS} } }
  }
}`

export const CARD_REPORT_FIELDS = `paymentMethodId month spending plannedSpending transactionCount performance projectedPerformance
  previousPerformance performanceSteps target remainingToTarget currentLimit achievedTier { minSpending benefitLimit }
  nextTier { minSpending benefitLimit } remainingToNextTier benefitTotal discountTotal pointTotal mileageTotal
  limitUsed benefitRemaining
  benefits { cardBenefitId name kind rate monthlyCap sharesCardLimit stackable spending count raw amount remaining spendToMax }
  transactionBenefits { transactionId kind amount }`

export const CARDS_MONTHLY_QUERY = `query CardsMonthly($month: YearMonth!) {
  cardsMonthly(month: $month) { ${CARD_REPORT_FIELDS} }
}`

export const CARD_DETAIL_QUERY = `query CardDetail($id: Int!, $month: YearMonth!) {
  paymentMethod(id: $id) {
    ${PAYMENT_METHOD_FIELDS}
    transactionCount lastUsedOn
    account { id name }
    nextPayment { ${STATEMENT_FIELDS} }
    statements { ${STATEMENT_FIELDS} }
    monthReport(month: $month) { ${CARD_REPORT_FIELDS} }
  }
}`

export const STATEMENT_LINES_QUERY = `query StatementLines($id: Int!, $month: YearMonth!) {
  statementLines(paymentMethodId: $id, month: $month) {
    amount installmentNo installmentMonths planned
    transaction { ${TX_FIELDS} }
  }
}`

export const CARD_LIST_QUERY = `query CardList($includeArchived: Boolean!) {
  paymentMethods(includeArchived: $includeArchived, cardsOnly: true) {
    ${PAYMENT_METHOD_FIELDS}
    transactionCount lastUsedOn
    account { id name }
    nextPayment { ${STATEMENT_FIELDS} }
  }
}`

export const RECURRING_QUERY = `query Recurring($ledgerId: Int) {
  recurringRules(ledgerId: $ledgerId) {
    id ledgerId description amount categoryId paymentMethodId dayOfMonth intervalMonths startMonth endMonth
    member memo matchKeyword matchAmountMin matchAmountMax excludeFromPerformance transferLedgerId active nextDate
  }
}`

export const PLAN_ITEMS_QUERY = `query PlanItems($ledgerId: Int) {
  planItems(ledgerId: $ledgerId) { id ledgerId kind name amount member sortOrder }
}`

export const ACCOUNT_HISTORY_QUERY = `query AccountHistory($id: Int!) {
  account(id: $id) { id history(limit: 60) { id accountId balance balanceDate memo createdAt } }
}`

export const SUGGESTIONS_QUERY = `query Suggestions($query: String!, $ledgerId: Int, $since: NaiveDate, $byCategory: Boolean) {
  descriptionSuggestions(query: $query, ledgerId: $ledgerId, limit: 8, since: $since, byCategory: $byCategory) {
    description ledgerId categoryId paymentMethodId member benefitIds lastAmount lastDate count
  }
}`

export const PARSE_MESSAGE_QUERY = `query Parse($text: String!) {
  parseMessage(text: $text) { amount merchant date time installmentMonths cancel issuer last4 }
}`

export const TOKENS_QUERY = `query Tokens { apiTokens { id name tokenPrefix scope createdAt lastUsedAt } }`

// ---------- 뮤테이션 ----------

export const CREATE_TX = `mutation CreateTx($input: TransactionInput!) { createTransaction(input: $input) { ${TX_FIELDS} } }`
export const UPDATE_TX = `mutation UpdateTx($id: Int!, $input: TransactionPatch!) { updateTransaction(id: $id, input: $input) { ${TX_FIELDS} } }`
export const DELETE_TXS = `mutation DeleteTxs($ids: [Int!]!) { deleteTransactions(ids: $ids) { deleted skipped } }`
export const SPLIT_TX = `mutation SplitTx($id: Int!, $ledgerId: Int!, $amount: Int!, $categoryId: Int) {
  splitTransaction(id: $id, ledgerId: $ledgerId, amount: $amount, categoryId: $categoryId) { id amount ledgerId }
}`
export const UNSPLIT_TX = `mutation UnsplitTx($id: Int!) { unsplitTransaction(id: $id) { id amount ledgerId } }`
export const MOVE_TXS = `mutation MoveTxs($ids: [Int!]!, $ledgerId: Int!) { moveTransactions(ids: $ids, ledgerId: $ledgerId) { moved uncategorized } }`
export const CONFIRM_TX = `mutation ConfirmTx($id: Int!, $date: NaiveDate, $amount: Int) { confirmTransaction(id: $id, date: $date, amount: $amount) { id status } }`
export const SKIP_TX = `mutation SkipTx($id: Int!) { skipTransaction(id: $id) { id status } }`
export const RESTORE_TX = `mutation RestoreTx($id: Int!) { restoreTransaction(id: $id) { id status } }`

export const CREATE_PM = `mutation CreatePm($input: PaymentMethodInput!) { createPaymentMethod(input: $input) { id } }`
export const UPDATE_PM = `mutation UpdatePm($id: Int!, $input: PaymentMethodInput!) { updatePaymentMethod(id: $id, input: $input) { id } }`
export const DELETE_PM = `mutation DeletePm($id: Int!) { deletePaymentMethod(id: $id) { deleted archived } }`
export const RESTORE_PM = `mutation RestorePm($id: Int!) { restorePaymentMethod(id: $id) { id } }`
export const SUGGEST_BILLING = `query SuggestBilling($day: Int!) { suggestedBilling(paymentDay: $day) { paymentDay closingDay closingMonthOffset usagePeriodLabel } }`

export const CREATE_ACCOUNT = `mutation CreateAccount($input: AccountInput!) { createAccount(input: $input) { id } }`
export const UPDATE_ACCOUNT = `mutation UpdateAccount($id: Int!, $input: AccountInput!) { updateAccount(id: $id, input: $input) { id } }`
export const UPDATE_BALANCE = `mutation UpdateBalance($id: Int!, $balance: Int!, $date: NaiveDate, $memo: String) {
  updateAccountBalance(id: $id, balance: $balance, date: $date, memo: $memo) { id balance balanceDate }
}`
export const DELETE_ACCOUNT = `mutation DeleteAccount($id: Int!) { deleteAccount(id: $id) }`
export const DELETE_BALANCE = `mutation DeleteBalance($id: Int!) { deleteAccountBalance(id: $id) }`

export const CREATE_CATEGORY = `mutation CreateCategory($input: CategoryInput!) { createCategory(input: $input) { id } }`
export const UPDATE_CATEGORY = `mutation UpdateCategory($id: Int!, $input: CategoryPatch!) { updateCategory(id: $id, input: $input) { id } }`
export const DELETE_CATEGORY = `mutation DeleteCategory($id: Int!) { deleteCategory(id: $id) { deleted archived } }`
export const REORDER_CATEGORIES = `mutation ReorderCategories($ids: [Int!]!) { reorderCategories(ids: $ids) }`

export const CREATE_PLAN_ITEM = `mutation CreatePlanItem($input: PlanItemInput!) { createPlanItem(input: $input) { id } }`
export const UPDATE_PLAN_ITEM = `mutation UpdatePlanItem($id: Int!, $input: PlanItemInput!) { updatePlanItem(id: $id, input: $input) { id } }`
export const DELETE_PLAN_ITEM = `mutation DeletePlanItem($id: Int!) { deletePlanItem(id: $id) }`

export const CREATE_RULE = `mutation CreateRule($input: RecurringRuleInput!) { createRecurringRule(input: $input) { id } }`
export const UPDATE_RULE = `mutation UpdateRule($id: Int!, $input: RecurringRuleInput!) { updateRecurringRule(id: $id, input: $input) { id } }`
export const DELETE_RULE = `mutation DeleteRule($id: Int!) { deleteRecurringRule(id: $id) }`

export const CREATE_LEDGER = `mutation CreateLedger($input: LedgerInput!) { createLedger(input: $input) { id } }`
export const UPDATE_LEDGER = `mutation UpdateLedger($id: Int!, $input: LedgerInput!) { updateLedger(id: $id, input: $input) { id } }`
export const DELETE_LEDGER = `mutation DeleteLedger($id: Int!) { deleteLedger(id: $id) }`

export const CREATE_TOKEN = `mutation CreateToken($name: String!, $scope: TokenScope! = INGEST) {
  createApiToken(name: $name, scope: $scope) { token apiToken { id name tokenPrefix scope createdAt lastUsedAt } }
}`
export const DELETE_TOKEN = `mutation DeleteToken($id: Int!) { deleteApiToken(id: $id) }`

export const INGEST = `mutation Ingest($input: IngestInput!) {
  ingestTransaction(input: $input) { created duplicate matchedPlanned warnings transaction { ${TX_FIELDS} } }
}`
