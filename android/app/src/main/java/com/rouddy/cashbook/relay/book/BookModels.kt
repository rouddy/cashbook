package com.rouddy.cashbook.relay.book

import kotlinx.serialization.Serializable

// 가계부 GraphQL 응답 (필요한 필드만). 이름은 서버 스키마(camelCase)와 같다 — 서버 필드를 바꾸면 [Queries]와 함께 맞춘다.

@Serializable
data class Ledger(val id: Int, val name: String, val isDefault: Boolean = false, val sortOrder: Int = 0)

@Serializable
data class Category(
    val id: Int,
    val ledgerId: Int,
    val name: String,
    /** EXPENSE, INCOME */
    val kind: String = "EXPENSE",
    val archived: Boolean = false,
) {
    val isIncome: Boolean get() = kind == "INCOME"
}

@Serializable
data class Benefit(
    val id: Int,
    val name: String,
    /** DISCOUNT, POINT, MILEAGE */
    val kind: String = "DISCOUNT",
    /** 다른 혜택과 함께 받을 수 있는 혜택. 아니면 거래당 하나만 */
    val stackable: Boolean = false,
)

@Serializable
data class PaymentMethod(
    val id: Int,
    val name: String,
    /** CREDIT_CARD, CHECK_CARD, CASH, BANK_TRANSFER, OTHER */
    val kind: String = "OTHER",
    val isCard: Boolean = false,
    val archived: Boolean = false,
    /** 확정 거래가 있는 마지막 날 (yyyy-MM-dd), 쓴 적 없으면 null */
    val lastUsedOn: String? = null,
    val benefits: List<Benefit> = emptyList(),
) {
    val isCreditCard: Boolean get() = kind == "CREDIT_CARD"
}

/** 화면마다 같이 쓰는 작은 목록 (장부·카테고리·결제수단·구성원). 보관한 항목도 이름 표시용으로 담는다. */
@Serializable
data class RefData(
    val ledgers: List<Ledger> = emptyList(),
    val categories: List<Category> = emptyList(),
    val paymentMethods: List<PaymentMethod> = emptyList(),
    val members: List<String> = emptyList(),
) {
    val defaultLedger: Ledger? get() = ledgers.firstOrNull { it.isDefault } ?: ledgers.firstOrNull()

    fun ledger(id: Int?): Ledger? = id?.let { i -> ledgers.firstOrNull { it.id == i } }

    fun category(id: Int?): Category? = id?.let { i -> categories.firstOrNull { it.id == i } }

    fun paymentMethod(id: Int?): PaymentMethod? = id?.let { i -> paymentMethods.firstOrNull { it.id == i } }

    /** 입력에서 고를 수 있는 카테고리: 그 장부의 사용 중인 지출(수입) 카테고리 */
    fun categoriesFor(ledgerId: Int?, income: Boolean): List<Category> =
        categories.filter { !it.archived && it.ledgerId == ledgerId && it.isIncome == income }

    val activePaymentMethods: List<PaymentMethod> get() = paymentMethods.filter { !it.archived }
}

@Serializable
data class Tx(
    val id: Int,
    val ledgerId: Int,
    /** yyyy-MM-dd */
    val date: String,
    val amount: Long,
    val categoryId: Int? = null,
    val paymentMethodId: Int? = null,
    val description: String = "",
    val memo: String? = null,
    val member: String? = null,
    /** CONFIRMED, PLANNED, SKIPPED */
    val status: String = "CONFIRMED",
    val installmentMonths: Int = 1,
    /** MANUAL, IMPORT, API(자동 입력), RECURRING */
    val source: String = "MANUAL",
    val rawText: String? = null,
    val isIncome: Boolean = false,
    /** 장부 이체면 받는 장부 */
    val transferLedgerId: Int? = null,
    val splitParentId: Int? = null,
    val benefitIds: List<Int> = emptyList(),
    val createdAt: String? = null,
) {
    val isTransfer: Boolean get() = transferLedgerId != null
    val isPlanned: Boolean get() = status == "PLANNED"
    val isAuto: Boolean get() = source == "API"
}

@Serializable
data class TxPage(val totalCount: Long = 0, val items: List<Tx> = emptyList())

@Serializable
data class CategorySummary(
    val categoryId: Int? = null,
    val ledgerId: Int? = null,
    val name: String = "",
    val kind: String = "EXPENSE",
    val confirmed: Long = 0,
    val planned: Long = 0,
    val count: Int = 0,
    val average: Double = 0.0,
    val budget: Long? = null,
)

@Serializable
data class DailyPoint(
    val day: Int,
    val date: String = "",
    val spending: Long = 0,
    val cumulative: Long = 0,
    /** 남은 예정 지출까지 더한 누적 */
    val projectedCumulative: Long = 0,
    val averageCumulative: Double = 0.0,
)

/** 서버 `domain/summary.rs`의 MonthlySummary (월잔액·지출계획은 장부를 정해야 계산된다) */
@Serializable
data class MonthlySummary(
    val month: String,
    val ledgerId: Int? = null,
    val averageMonthsUsed: Int = 0,
    val spending: Long = 0,
    val plannedSpending: Long = 0,
    val income: Long = 0,
    val plannedIncome: Long = 0,
    val transferIn: Long = 0,
    val plannedTransferIn: Long = 0,
    val transferOut: Long = 0,
    val plannedTransferOut: Long = 0,
    val averageSpending: Double = 0.0,
    val budgetTotal: Long? = null,
    val balance: Long? = null,
    val projectedBalance: Long? = null,
    val categories: List<CategorySummary> = emptyList(),
    val daily: List<DailyPoint> = emptyList(),
)

@Serializable
data class Tier(val minSpending: Long = 0, val benefitLimit: Long = 0)

/** 서버 `domain/card_report.rs`의 CardMonthReport */
@Serializable
data class CardReport(
    val paymentMethodId: Int,
    val spending: Long = 0,
    val plannedSpending: Long = 0,
    val performance: Long = 0,
    val projectedPerformance: Long = 0,
    /** 실적 구간 경계 (마지막 값이 막대 끝) */
    val performanceSteps: List<Long> = emptyList(),
    val target: Long? = null,
    val remainingToTarget: Long? = null,
    val currentLimit: Long? = null,
    val limitUsed: Long = 0,
    val discountTotal: Long = 0,
    val pointTotal: Long = 0,
    val mileageTotal: Long = 0,
    val achievedTier: Tier? = null,
    val nextTier: Tier? = null,
    val remainingToNextTier: Long? = null,
) {
    /** 예정 거래가 이번 달 실적에 더하는 금액 (웹 `plannedPerformance`) */
    val plannedPerformance: Long get() = projectedPerformance - performance

    /** 웹 대시보드와 같은 기준: 전월실적(목표·구간)이 있는 카드만. 서버는 실적 단계가 있을 때만 target을 채운다 */
    val worthShowing: Boolean get() = target != null
}

@Serializable
data class Statement(
    val month: String = "",
    val periodStart: String = "",
    val periodEnd: String = "",
    val paymentDate: String = "",
    val confirmedAmount: Long = 0,
    val plannedAmount: Long = 0,
    val closed: Boolean = false,
    val paid: Boolean = false,
)

@Serializable
data class CardDue(
    val paymentMethodId: Int,
    /** false면 여유자금 제외 계좌에서 나가 여유자금에서 빼지 않는다 */
    val counted: Boolean = true,
    val nextPayment: Statement? = null,
)

/** 서버 `domain/funds.rs`의 AvailableFunds */
@Serializable
data class Funds(
    val today: String = "",
    val accountsTotal: Long = 0,
    val balanceDate: String? = null,
    val cardDue: Long = 0,
    val cardDuePlanned: Long = 0,
    val unreflectedOutflow: Long = 0,
    val plannedOutflow: Long = 0,
    val plannedInflow: Long = 0,
    val available: Long = 0,
    val availableAfterPlanned: Long = 0,
    val cards: List<CardDue> = emptyList(),
)

@Serializable
data class Dashboard(
    val monthlySummary: MonthlySummary,
    val cardsMonthly: List<CardReport> = emptyList(),
    val availableFunds: Funds? = null,
    /** 지난 미확정 + 앞으로 며칠의 예정 거래 */
    val planned: TxPage = TxPage(),
)

/** 내용 입력 중 고를 수 있는 과거 가맹점과 그때의 분류 */
@Serializable
data class Suggestion(
    val description: String,
    val ledgerId: Int,
    val categoryId: Int? = null,
    val paymentMethodId: Int? = null,
    val member: String? = null,
    val benefitIds: List<Int> = emptyList(),
    val lastAmount: Long = 0,
    val lastDate: String? = null,
    val count: Int = 0,
)

@Serializable
data class SavedTx(val id: Int, val description: String = "", val amount: Long = 0, val status: String = "CONFIRMED")
