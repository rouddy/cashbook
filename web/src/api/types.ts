// 서버 GraphQL 스키마와 맞춘 타입 (필요한 필드만).

export type TxStatus = 'CONFIRMED' | 'PLANNED' | 'SKIPPED'
export type TxSource = 'MANUAL' | 'IMPORT' | 'API' | 'RECURRING'
export type CategoryKind = 'EXPENSE' | 'INCOME'
export type PaymentKind = 'CREDIT_CARD' | 'CHECK_CARD' | 'CASH' | 'BANK_TRANSFER' | 'OTHER'
export type AccountKind = 'CHECKING' | 'SAVINGS' | 'DEPOSIT' | 'INVESTMENT' | 'CASH' | 'OTHER'
/** 배분(저축 등) / 구성원 용돈. 수입은 정기수익 반복 규칙, 다른 장부로 보내는 돈은 장부 이체 반복 규칙 */
export type PlanItemKind = 'ALLOCATION' | 'MEMBER_ALLOWANCE'
/** 할인 / 포인트 적립 / 마일리지 적립 */
export type BenefitKind = 'DISCOUNT' | 'POINT' | 'MILEAGE'

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
  color: string | null
  sortOrder: number
  archived: boolean
}

export interface CardBenefit {
  id: number
  paymentMethodId: number
  name: string
  kind: BenefitKind
  /** 이용금액 1원당 혜택 (할인·포인트 0.1 = 10%, 마일리지 1/1500 = 1,500원당 1마일) */
  rate: number
  minAmount: number | null
  perTxMax: number | null
  monthlyCap: number | null
  /** 통합 한도에서 차감. 아니고 monthlyCap도 없으면 한도 없음 */
  sharesCardLimit: boolean
  excludesPerformance: boolean
  /** 다른 혜택과 함께 받을 수 있음. 아니면 거래당 하나만 */
  stackable: boolean
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
  /** 분할해 옮긴 거래면 원래 거래 id */
  splitParentId: number | null
  /** 장부 이체면 받는 장부 (보내는 장부는 ledgerId, 항목·결제수단 없음) */
  transferLedgerId: number | null
  /** 같은 결제를 나눈 다른 장부의 거래들 (분할하지 않았으면 빈 목록) */
  splitWith: SplitPart[]
}

/** 분할한 결제의 다른 쪽. 이 거래와 금액을 합치면 결제 금액이다. */
export interface SplitPart {
  id: number
  ledgerId: number
  amount: number
  categoryId: number | null
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
  /** 장부를 정한 목록에서 그 장부가 받은·보낸 이체 (지출·수입 합계와 별도) */
  transferInTotal: number
  plannedTransferInTotal: number
  transferOutTotal: number
  plannedTransferOutTotal: number
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
  income: number
  fixedSpending: number
}

/** 상대 장부·방향별 장부 이체 합계 */
export interface TransferSummary {
  /** 받은 이체면 보낸 장부, 보낸 이체면 받은 장부 */
  ledgerId: number
  incoming: boolean
  confirmed: number
  planned: number
  count: number
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
  income: number
  plannedIncome: number
  transferIn: number
  plannedTransferIn: number
  transferOut: number
  plannedTransferOut: number
  averageSpending: number
  averageFixedSpending: number
  averageVariableSpending: number
  budgetTotal: number | null
  plan: { allocations: number; memberAllowances: number; items: PlanItem[] } | null
  balance: number | null
  projectedBalance: number | null
  categories: CategorySummary[]
  paymentMethods: PaymentMethodSummary[]
  transfers: TransferSummary[]
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
  kind: BenefitKind
  rate: number
  monthlyCap: number | null
  sharesCardLimit: boolean
  stackable: boolean
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
  /** 전월실적 단계 (실적 목표 + 구간 기준 금액, 오름차순) */
  performanceSteps: number[]
  /** 지금 실적의 목표 단계: 아직 못 넘은 가장 낮은 단계, 다 넘었으면 마지막 단계 */
  target: number | null
  remainingToTarget: number | null
  currentLimit: number | null
  achievedTier: TierInfo | null
  nextTier: TierInfo | null
  remainingToNextTier: number | null
  /** 할인 + 포인트 (원) */
  benefitTotal: number
  discountTotal: number
  pointTotal: number
  mileageTotal: number
  /** 통합 한도에서 차감된 혜택 */
  limitUsed: number
  benefitRemaining: number | null
  benefits: BenefitUsage[]
  /** 거래·혜택 종류별 혜택액 (종류의 단위) */
  transactionBenefits: { transactionId: number; kind: BenefitKind; amount: number }[]
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
  /** 자동 입력 매칭 키워드 (쉼표로 여러 개). 정했으면 가맹점에 하나는 있어야 확정 */
  matchKeyword: string | null
  /** 자동 입력 매칭 금액 범위 (양끝 포함, 한쪽만 정해도 됨). 정했으면 실제 금액이 이 안이어야 확정 */
  matchAmountMin: number | null
  matchAmountMax: number | null
  excludeFromPerformance: boolean
  /** 장부 이체 규칙이면 받는 장부 */
  transferLedgerId: number | null
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
