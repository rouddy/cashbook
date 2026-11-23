package com.rouddy.cashbook.relay.book

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.time.LocalDate

/** 수동 입력 종류. 금액 부호: 지출 +(환불이면 −), 수입 +, 이체 +(보내는 장부 → 받는 장부) */
enum class EntryKind(val label: String) {
    EXPENSE("지출"),
    INCOME("수입"),
    TRANSFER("이체"),
}

/** 수동 입력 화면의 값. [amount]는 사용자가 친 글자 그대로(쉼표 포함 가능) */
data class EntryForm(
    val kind: EntryKind = EntryKind.EXPENSE,
    val amount: String = "",
    val refund: Boolean = false,
    val description: String = "",
    val date: LocalDate,
    /** 지출·수입의 장부, 이체의 보내는 장부 */
    val ledgerId: Int? = null,
    /** 이체의 받는 장부 */
    val toLedgerId: Int? = null,
    val categoryId: Int? = null,
    val paymentMethodId: Int? = null,
    val installmentMonths: Int = 1,
    val benefitIds: List<Int> = emptyList(),
    val excludeFromPerformance: Boolean = false,
    val member: String = "",
    val memo: String = "",
    val planned: Boolean = false,
)

/** 수동 입력 규칙. 서버 `createTransaction`의 검증(`graphql/transaction.rs`의 `create_one`)과 같게 맞춘다. */
object EntryRules {
    const val TRANSFER_DEFAULT_DESCRIPTION = "장부 이체"
    val INSTALLMENT_CHOICES = listOf(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 18, 24)

    fun parseAmount(text: String): Long? = text.filter(Char::isDigit).takeIf { it.isNotEmpty() && it.length <= 13 }?.toLong()

    fun newForm(ref: RefData?, today: LocalDate): EntryForm {
        val ledger = ref?.defaultLedger?.id
        return EntryForm(date = today, ledgerId = ledger, toLedgerId = otherLedger(ref, ledger))
    }

    /** 이체 받는 장부 기본값: 보내는 장부가 아닌 첫 장부 */
    private fun otherLedger(ref: RefData?, from: Int?): Int? = ref?.ledgers?.firstOrNull { it.id != from }?.id

    fun withKind(form: EntryForm, kind: EntryKind, ref: RefData): EntryForm {
        if (form.kind == kind) return form
        val category = ref.category(form.categoryId)?.takeIf { kind != EntryKind.TRANSFER && it.isIncome == (kind == EntryKind.INCOME) }
        val base = form.copy(kind = kind, categoryId = category?.id, refund = form.refund && kind == EntryKind.EXPENSE)
        return when (kind) {
            EntryKind.TRANSFER -> base.copy(
                paymentMethodId = null,
                benefitIds = emptyList(),
                installmentMonths = 1,
                excludeFromPerformance = false,
                toLedgerId = form.toLedgerId?.takeIf { it != form.ledgerId } ?: otherLedger(ref, form.ledgerId),
            )
            // 혜택·할부·실적 제외는 카드 지출에만
            EntryKind.INCOME -> base.copy(benefitIds = emptyList(), installmentMonths = 1, excludeFromPerformance = false)
            EntryKind.EXPENSE -> base
        }
    }

    /** 장부를 바꾸면 그 장부에 없는 카테고리는 비우고, 이체 받는 장부가 같아지면 다른 장부로 */
    fun withLedger(form: EntryForm, ledgerId: Int, ref: RefData): EntryForm {
        val category = ref.category(form.categoryId)?.takeIf { it.ledgerId == ledgerId }
        val to = form.toLedgerId?.takeIf { it != ledgerId } ?: otherLedger(ref, ledgerId)
        return form.copy(ledgerId = ledgerId, categoryId = category?.id, toLedgerId = to)
    }

    /** 결제수단 드롭다운의 종류 순서 ('선택 안 함' 다음). 여기 없는 종류는 맨 뒤 */
    private val PAYMENT_KIND_ORDER = listOf("CASH", "CREDIT_CARD", "CHECK_CARD", "BANK_TRANSFER", "OTHER")

    /**
     * 고를 수 있는 결제수단 순서: 현금 → 신용카드 → 체크카드 → 계좌이체 → 기타.
     * 신용카드는 최근에 쓴 카드부터(쓴 적 없는 카드는 뒤로), 그 밖은 서버 순서(정렬 순서·id)를 지킨다.
     */
    fun paymentMethodChoices(ref: RefData): List<PaymentMethod> {
        fun kindRank(pm: PaymentMethod) = PAYMENT_KIND_ORDER.indexOf(pm.kind).let { if (it < 0) PAYMENT_KIND_ORDER.size else it }
        // yyyy-MM-dd라 글자 순서가 날짜 순서. 내림차순에서 null(쓴 적 없음)은 뒤로 간다
        return ref.activePaymentMethods.sortedWith(
            compareBy<PaymentMethod> { kindRank(it) }.thenByDescending { if (it.isCreditCard) it.lastUsedOn else null },
        )
    }

    /** 결제수단을 바꾸면 다른 카드의 혜택은 버리고, 신용카드가 아니면 일시불로 */
    fun withPaymentMethod(form: EntryForm, paymentMethodId: Int?, ref: RefData): EntryForm {
        val pm = ref.paymentMethod(paymentMethodId)
        val valid = pm?.benefits?.map { it.id }?.toSet().orEmpty()
        return form.copy(
            paymentMethodId = pm?.id,
            benefitIds = form.benefitIds.filter { it in valid },
            installmentMonths = if (pm?.isCreditCard == true) form.installmentMonths else 1,
            excludeFromPerformance = form.excludeFromPerformance && pm?.isCard == true,
        )
    }

    /** 웹 `toggleBenefit`과 같음: 중복 적용이 아닌 혜택은 거래당 하나만 */
    fun toggleBenefit(selected: List<Int>, benefit: Benefit, checked: Boolean, all: List<Benefit>): List<Int> {
        val rest = selected.filter { it != benefit.id }
        if (!checked) return rest
        if (benefit.stackable) return rest + benefit.id
        val exclusive = all.filterNot { it.stackable }.map { it.id }.toSet()
        return rest.filterNot { it in exclusive } + benefit.id
    }

    /** 과거 가맹점을 고르면 분류를 채운다 (웹 결제 입력의 `pick`과 같음). 수입 카테고리면 수입으로 바꾼다. */
    fun applySuggestion(form: EntryForm, s: Suggestion, ref: RefData): EntryForm {
        val category = ref.category(s.categoryId)?.takeIf { !it.archived }
        val kind = when {
            form.kind == EntryKind.TRANSFER -> EntryKind.EXPENSE
            category != null -> if (category.isIncome) EntryKind.INCOME else EntryKind.EXPENSE
            else -> form.kind
        }
        val pmId = form.paymentMethodId ?: s.paymentMethodId?.takeIf { id -> ref.paymentMethod(id)?.archived == false }
        val samePm = form.paymentMethodId == null || form.paymentMethodId == s.paymentMethodId
        val filled = form.copy(
            kind = kind,
            description = s.description,
            ledgerId = ref.ledger(s.ledgerId)?.id ?: form.ledgerId,
            categoryId = category?.id,
            member = form.member.ifBlank { s.member.orEmpty() },
            benefitIds = if (samePm) s.benefitIds else form.benefitIds,
            amount = form.amount.ifBlank { if (s.lastAmount > 0) s.lastAmount.toString() else "" },
            refund = form.refund && kind == EntryKind.EXPENSE,
        )
        return withPaymentMethod(filled, pmId, ref).let {
            if (kind == EntryKind.EXPENSE) it else it.copy(benefitIds = emptyList(), installmentMonths = 1, excludeFromPerformance = false)
        }
    }

    /** 저장한 뒤 다음 입력: 종류·날짜·장부·결제수단·상태는 그대로 둔다 */
    fun afterSave(form: EntryForm): EntryForm = EntryForm(
        kind = form.kind,
        date = form.date,
        ledgerId = form.ledgerId,
        toLedgerId = form.toLedgerId,
        paymentMethodId = form.paymentMethodId,
        planned = form.planned,
    )

    /** `createTransaction`의 `TransactionInput`. 고칠 것이 있으면 사용자에게 보여 줄 메시지로 실패 */
    fun toInput(form: EntryForm, ref: RefData): Result<JsonObject> {
        val amount = parseAmount(form.amount)
        if (amount == null || amount == 0L) return Result.failure(IllegalArgumentException("금액을 입력하세요"))
        val ledger = ref.ledger(form.ledgerId) ?: return Result.failure(IllegalArgumentException("장부를 고르세요"))
        val status = if (form.planned) "PLANNED" else "CONFIRMED"
        val memo = form.memo.trim().ifEmpty { null }
        val member = form.member.trim().ifEmpty { null }
        if (form.kind == EntryKind.TRANSFER) {
            val to = ref.ledger(form.toLedgerId) ?: return Result.failure(IllegalArgumentException("받는 장부를 고르세요"))
            if (to.id == ledger.id) return Result.failure(IllegalArgumentException("받는 장부는 보내는 장부와 달라야 합니다"))
            return Result.success(
                buildJsonObject {
                    put("ledgerId", ledger.id)
                    put("transferLedgerId", to.id)
                    put("date", form.date.toString())
                    put("amount", amount)
                    put("description", form.description.trim().ifEmpty { TRANSFER_DEFAULT_DESCRIPTION })
                    put("memo", memo)
                    put("member", member)
                    put("status", status)
                },
            )
        }
        val description = form.description.trim()
        if (description.isEmpty()) return Result.failure(IllegalArgumentException("내용을 입력하세요"))
        val category = ref.category(form.categoryId)
        if (category != null && (category.ledgerId != ledger.id || category.isIncome != (form.kind == EntryKind.INCOME))) {
            return Result.failure(IllegalArgumentException("'${category.name}' 항목을 다시 고르세요"))
        }
        val pm = ref.paymentMethod(form.paymentMethodId)
        val expense = form.kind == EntryKind.EXPENSE
        val benefits = if (expense && pm != null) form.benefitIds.filter { id -> pm.benefits.any { it.id == id } } else emptyList()
        return Result.success(
            buildJsonObject {
                put("ledgerId", ledger.id)
                put("date", form.date.toString())
                put("amount", if (expense && form.refund) -amount else amount)
                put("categoryId", category?.id?.let(::JsonPrimitive) ?: JsonNull)
                put("paymentMethodId", pm?.id?.let(::JsonPrimitive) ?: JsonNull)
                put("description", description)
                put("memo", memo)
                put("member", member)
                put("status", status)
                put("installmentMonths", if (expense && pm?.isCreditCard == true) form.installmentMonths else 1)
                put("excludeFromPerformance", expense && pm?.isCard == true && form.excludeFromPerformance)
                put("benefitIds", JsonArray(benefits.map(::JsonPrimitive)))
            },
        )
    }
}
