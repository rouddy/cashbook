package com.rouddy.cashbook.relay.book

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** 최근 내역에서 고치는 값: 항목·카드 혜택·메모 */
data class TxEdit(
    val categoryId: Int? = null,
    val benefitIds: List<Int> = emptyList(),
    val memo: String = "",
)

/**
 * 최근 내역의 거래 고치기. 서버 `updateTransaction`(`graphql/transaction.rs`)에 바뀐 필드만 보낸다.
 * - 항목은 거래 장부의 것만 고른다: 서버는 다른 장부의 항목을 받으면 거래를 그 장부로 옮기고(분할 묶음은 거절), 그건 웹에서 한다.
 * - 장부 이체는 메모만 고친다 (이체를 푸는 건 웹에서).
 * - 혜택은 지출의 카드 혜택만 (수동 입력과 같음). 수입 항목으로 바꾸면 고른 혜택을 지운다.
 *   분할한 거래면 서버가 원래 거래(부모)의 혜택을 바꾼다 — 목록의 [Tx.benefitIds]도 부모 것이다.
 */
object TxEditRules {
    fun of(tx: Tx): TxEdit = TxEdit(categoryId = tx.categoryId, benefitIds = tx.benefitIds, memo = tx.memo.orEmpty())

    /** 고를 수 있는 항목: 거래 장부의 사용 중인 항목(지출·수입) + 지금 항목이 보관됐으면 그것도 */
    fun categoryChoices(tx: Tx, ref: RefData): List<Category> {
        if (tx.isTransfer) return emptyList()
        return ref.categories.filter { it.ledgerId == tx.ledgerId && (!it.archived || it.id == tx.categoryId) }
    }

    /** 혜택을 고를 카드. 장부 이체·수입 항목·혜택이 없는 결제수단이면 null */
    fun benefitCard(tx: Tx, edit: TxEdit, ref: RefData): PaymentMethod? {
        if (tx.isTransfer || ref.category(edit.categoryId)?.isIncome == true) return null
        return ref.paymentMethod(tx.paymentMethodId)?.takeIf { it.benefits.isNotEmpty() }
    }

    /**
     * 보낼 혜택 목록. 혜택을 고쳤거나, 항목을 바꿔 혜택을 고를 수 없게 됐을 때(수입 항목)만 보낸다 —
     * 메모만 고칠 때 예전 데이터(수입 거래의 혜택 등)를 건드리지 않게.
     */
    private fun benefitsToSave(tx: Tx, edit: TxEdit, ref: RefData): List<Int>? {
        val card = benefitCard(tx, edit, ref)
        val toggled = edit.benefitIds.toSet() != tx.benefitIds.toSet()
        val dropped = card == null && tx.benefitIds.isNotEmpty() && edit.categoryId != tx.categoryId
        if (!toggled && !dropped) return null
        return card?.let { c -> edit.benefitIds.filter { id -> c.benefits.any { it.id == id } } }.orEmpty()
    }

    private fun normalizedMemo(memo: String?): String? = memo?.trim()?.ifEmpty { null }

    /**
     * `TransactionPatch` (바뀐 필드만, 바뀐 게 없으면 빈 객체). 고칠 것이 있으면 사용자에게 보여 줄 메시지로 실패.
     * 서버가 메모를 trim하고 빈 값은 null로 두므로 같은 기준으로 비교한다.
     */
    fun toPatch(tx: Tx, edit: TxEdit, ref: RefData): Result<JsonObject> {
        val fields = linkedMapOf<String, JsonElement>()
        if (!tx.isTransfer && edit.categoryId != tx.categoryId) {
            val category = ref.category(edit.categoryId)
            if (edit.categoryId != null && (category == null || category.archived || category.ledgerId != tx.ledgerId)) {
                return Result.failure(IllegalArgumentException("항목을 다시 고르세요"))
            }
            fields["categoryId"] = category?.id?.let(::JsonPrimitive) ?: JsonNull
        }
        benefitsToSave(tx, edit, ref)?.let { ids -> fields["benefitIds"] = JsonArray(ids.map(::JsonPrimitive)) }
        val memo = normalizedMemo(edit.memo)
        if (memo != normalizedMemo(tx.memo)) fields["memo"] = memo?.let(::JsonPrimitive) ?: JsonNull
        return Result.success(JsonObject(fields))
    }

    /** 저장할 것이 있는지 (잘못 고른 값도 저장을 눌러 메시지를 보게 true) */
    fun changed(tx: Tx, edit: TxEdit, ref: RefData): Boolean = toPatch(tx, edit, ref).map { it.isNotEmpty() }.getOrDefault(true)
}
