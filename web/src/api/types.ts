// 서버 GraphQL 스키마와 맞춘 타입 (필요한 필드만).

export type TxStatus = 'CONFIRMED' | 'PLANNED' | 'SKIPPED'
export type TxSource = 'MANUAL' | 'IMPORT' | 'API' | 'RECURRING'
export type CategoryKind = 'EXPENSE' | 'INCOME'
export type PaymentKind = 'CREDIT_CARD' | 'CHECK_CARD' | 'CASH' | 'BANK_TRANSFER' | 'OTHER'
export type AccountKind = 'CHECKING' | 'SAVINGS' | 'DEPOSIT' | 'INVESTMENT' | 'CASH' | 'OTHER'
export type PlanItemKind = 'INCOME' | 'ALLOCATION' | 'MEMBER_ALLOWANCE'

export interface AppInfo {
  version: string
  today: string
  timezone: string
  authEnabled: boolean
  planMonthsAhead: number
}

export interface Ledger {
  id: number
  name: string
  description: string | null
  isDefault: boolean
  sortOrder: number
}

export interface Category {
  id: number
  ledgerId: number
  name: string
  kind: CategoryKind
  monthlyBudget: number | null
  isFixed: boolean
  excludedFromSpending: boolean
  color: string | null
  sortOrder: number
  archived: boolean
}

export interface CardBenefit {
  id: number
  paymentMethodId: number
  name: string
  rate: number
  minAmount: number | null
  perTxMax: number | null
  monthlyCap: number | null
  sharesCardLimit: boolean
  excludesPerformance: boolean
  sortOrder: number
}

export interface CardTier {
  id: number
  paymentMethodId: number
  minSpending: number
  benefitLimit: number
}

export interface PaymentMethod {
  id: number
  name: string
  kind: PaymentKind
  issuer: string | null
  productName: string | null
  last4: string | null
  aliases: string[]
  accountId: number | null
  paymentDay: number | null
  closingDay: number | null
  closingMonthOffset: number | null
  monthlyTarget: number | null
  annualFee: number | null
  expiresOn: string | null
  color: string | null
  memo: string | null
  sortOrder: number
  archived: boolean
  isCard: boolean
  billingConfigured: boolean
  usagePeriodLabel: string | null
  benefits: CardBenefit[]
  tiers: CardTier[]
}

export interface Account {
  id: number
  name: string
  bank: string | null
  kind: AccountKind
  accountNumber: string | null
  balance: number
  balanceDate: string | null
  includeInAvailable: boolean
  interestRate: number | null
  maturityDate: string | null
  memo: string | null
  sortOrder: number
  archived: boolean
  updatedAt: string
}

export interface AccountBalance {
  id: number
  accountId: number
  balance: number
  balanceDate: string
  memo: string | null
  createdAt: string
}

export interface Transaction {
  id: number
  ledgerId: number
  date: string
  amount: number
  categoryId: number | null
  paymentMethodId: number | null
  description: string
  memo: string | null
  member: string | null
  status: TxStatus
  installmentMonths: number
  excludeFromPerformance: boolean
  recurringRuleId: number | null
  recurringMonth: string | null
  source: TxSource
  rawText: string | null
  benefitIds: number[]
  statementMonth: string | null
  paymentDate: string
  isIncome: boolean
}

export interface TransactionPage {
  items: Transaction[]
  totalCount: number
  /** 확정 지출 (수입 카테고리 제외, 환불 차감) */
  confirmedTotal: number
  plannedTotal: number
  /** 확정 수입 (수입 카테고리) */
  incomeTotal: number
  plannedIncomeTotal: number
}

export interface StatementInfo {
  month: string
  periodStart: string
  periodEnd: string
  paymentDate: string
  confirmedAmount: number
  plannedAmount: number
  closed: boolean
  paid: boolean
}

export interface CategorySummary {
  categoryId: number | null
  ledgerId: number | null
  name: string
  kind: CategoryKind
  isFixed: boolean
  excludedFromSpending: boolean
  confirmed: number
  planned: number
  count: number
  average: number
  budget: number | null
}

export interface PaymentMethodSummary {
  paymentMethodId: number | null
  name: string
  confirmed: number
  planned: number
  count: number
}

export interface MemberSummary {
  member: string
  spending: number
  planned: number
  allowance: number | null
  remaining: number | null
}

export interface DailyPoint {
  day: number
  date: string
  spending: number
  cumulative: number
  projectedCumulative: number
  averageCumulative: number
}

export interface MonthPoint {
  month: string
  spending: number
  plannedSpending: number
  excludedSpending: number
  income: number
  fixedSpending: number
}

export interface PlanItem {
  id: number
  ledgerId: number
  kind: PlanItemKind
  name: string
  amount: number
  member: string | null
  sortOrder: number
}

export interface MonthlySummary {
  month: string
  ledgerId: number | null
  averageMonths: number
  averageMonthsUsed: number
  spending: number
  plannedSpending: number
  fixedSpending: number
  variableSpending: number
  excludedSpending: number
  plannedExcludedSpending: number
  income: number
  plannedIncome: number
  averageSpending: number
  averageFixedSpending: number
  averageVariableSpending: number
  budgetTotal: number | null
  plan: { income: number; allocations: number; memberAllowances: number; items: PlanItem[] } | null
  balance: number | null
  projectedBalance: number | null
  categories: CategorySummary[]
  paymentMethods: PaymentMethodSummary[]
  members: MemberSummary[]
  daily: DailyPoint[]
  history: MonthPoint[]
}

export interface CardDue {
  paymentMethodId: number
  billingConfigured: boolean
  /** 결제 계좌가 여유자금 계산에 포함되는지 */
  counted: boolean
  cutoffDate: string
  due: number
  duePlanned: number
  nextPayment: StatementInfo | null
  statements: StatementInfo[]
  recentSpending: number
}

export interface AvailableFunds {
  today: string
  accountsTotal: number
  balanceDate: string | null
  cardDue: number
  cardDuePlanned: number
  unreflectedOutflow: number
  plannedOutflow: number
  plannedInflow: number
  available: number
  availableAfterPlanned: number
  cards: CardDue[]
  unconfiguredCardIds: number[]
}

export interface TierInfo {
  minSpending: number
  benefitLimit: number
}

export interface BenefitUsage {
  cardBenefitId: number
  name: string
  rate: number
  monthlyCap: number | null
  sharesCardLimit: boolean
  spending: number
  count: number
  raw: number
  amount: number
  remaining: number | null
  spendToMax: number | null
}

export interface CardMonthReport {
  paymentMethodId: number
  month: string
  spending: number
  plannedSpending: number
  transactionCount: number
  performance: number
  projectedPerformance: number
  previousPerformance: number
  target: number | null
  remainingToTarget: number | null
  currentLimit: number | null
  achievedTier: TierInfo | null
  nextTier: TierInfo | null
  remainingToNextTier: number | null
  benefitTotal: number
  benefitRemaining: number | null
  benefits: BenefitUsage[]
  transactionBenefits: { transactionId: number; amount: number }[]
}

export interface StatementLine {
  transaction: Transaction
  amount: number
  installmentNo: number
  installmentMonths: number
  planned: boolean
}

export interface RecurringRule {
  id: number
  ledgerId: number
  description: string
  amount: number
  categoryId: number | null
  paymentMethodId: number | null
  dayOfMonth: number
  intervalMonths: number
  startMonth: string
  endMonth: string | null
  member: string | null
  memo: string | null
  matchKeyword: string | null
  excludeFromPerformance: boolean
  active: boolean
  nextDate: string | null
}

export interface Suggestion {
  description: string
  ledgerId: number
  categoryId: number | null
  paymentMethodId: number | null
  member: string | null
  benefitIds: number[]
  lastAmount: number
  lastDate: string
  count: number
}

export interface ParsedMessage {
  amount: number | null
  merchant: string | null
  date: string | null
  time: string | null
  installmentMonths: number | null
  cancel: boolean
  issuer: string | null
  last4: string | null
}

export type TokenScope = 'INGEST' | 'FULL'

export interface ApiToken {
  id: number
  name: string
  tokenPrefix: string
  scope: TokenScope
  createdAt: string
  lastUsedAt: string | null
}
