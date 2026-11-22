package com.rouddy.cashbook.relay

import com.rouddy.cashbook.relay.book.Benefit
import com.rouddy.cashbook.relay.book.Category
import com.rouddy.cashbook.relay.book.Ledger
import com.rouddy.cashbook.relay.book.PaymentMethod
import com.rouddy.cashbook.relay.book.RefData
import com.rouddy.cashbook.relay.book.Tx
import com.rouddy.cashbook.relay.book.TxEdit
import com.rouddy.cashbook.relay.book.TxEditRules
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class TxEditRulesTest {
    private val coffee = Benefit(7, "커피 10%")
    private val movie = Benefit(8, "영화 5천원")
    private val points = Benefit(9, "기본 적립", kind = "POINT", stackable = true)
    private val card = PaymentMethod(5, "신한 딥드림", kind = "CREDIT_CARD", isCard = true, benefits = listOf(coffee, movie, points))
    private val ref = RefData(
        ledgers = listOf(Ledger(1, "가계부", isDefault = true), Ledger(2, "용돈")),
        categories = listOf(
            Category(10, 1, "외식"),
            Category(11, 1, "월급", kind = "INCOME"),
            Category(12, 1, "예전 항목", archived = true),
            Category(13, 1, "쇼핑"),
            Category(20, 2, "외식"),
        ),
        paymentMethods = listOf(card, PaymentMethod(7, "현금", kind = "CASH")),
    )
    private val tx = Tx(
        id = 100,
        ledgerId = 1,
        date = "2026-11-21",
        amount = 12_500,
        categoryId = 10,
        paymentMethodId = 5,
        description = "스타벅스",
        memo = "회의",
        benefitIds = listOf(7),
    )

    private fun patch(t: Tx, edit: TxEdit): JsonObject = TxEditRules.toPatch(t, edit, ref).getOrThrow()

    @Test
    fun unchangedSendsNothing() {
        val edit = TxEditRules.of(tx)
        assertEquals(JsonObject(emptyMap()), patch(tx, edit))
        assertFalse(TxEditRules.changed(tx, edit, ref))
        // 앞뒤 공백만 바뀐 메모도 서버가 trim하므로 바뀐 게 아니다
        assertFalse(TxEditRules.changed(tx, edit.copy(memo = " 회의 "), ref))
    }

    @Test
    fun sendsOnlyChangedFields() {
        val base = TxEditRules.of(tx)
        assertEquals(JsonObject(mapOf("categoryId" to JsonPrimitive(13))), patch(tx, base.copy(categoryId = 13)))
        assertEquals(JsonObject(mapOf("memo" to JsonPrimitive("점심"))), patch(tx, base.copy(memo = " 점심 ")))
        assertEquals(
            JsonObject(mapOf("benefitIds" to JsonArray(listOf(JsonPrimitive(7), JsonPrimitive(9))))),
            patch(tx, base.copy(benefitIds = listOf(7, 9))),
        )
    }

    @Test
    fun clearingSendsNull() {
        val base = TxEditRules.of(tx)
        assertEquals(JsonNull, patch(tx, base.copy(categoryId = null))["categoryId"])
        assertEquals(JsonNull, patch(tx, base.copy(memo = "  "))["memo"])
        assertEquals(JsonArray(emptyList()), patch(tx, base.copy(benefitIds = emptyList()))["benefitIds"])
    }

    @Test
    fun benefitOrderDoesNotMatter() {
        val t = tx.copy(benefitIds = listOf(9, 7))
        assertFalse(TxEditRules.changed(t, TxEditRules.of(t).copy(benefitIds = listOf(7, 9)), ref))
    }

    @Test
    fun categoryChoicesAreThisLedgerOnlyAndKeepArchivedCurrent() {
        assertEquals(listOf(10, 11, 13), TxEditRules.categoryChoices(tx, ref).map { it.id })
        assertEquals(listOf(10, 11, 12, 13), TxEditRules.categoryChoices(tx.copy(categoryId = 12), ref).map { it.id })
        // 다른 장부의 항목을 주면 서버가 거래를 그 장부로 옮기므로 막는다
        assertTrue(TxEditRules.toPatch(tx, TxEditRules.of(tx).copy(categoryId = 20), ref).isFailure)
        assertTrue(TxEditRules.toPatch(tx, TxEditRules.of(tx).copy(categoryId = 12), ref).isFailure)
        // 보관된 지금 항목을 그대로 두면 보내지 않는다
        val archived = tx.copy(categoryId = 12)
        assertEquals(JsonObject(mapOf("memo" to JsonPrimitive("점심"))), patch(archived, TxEditRules.of(archived).copy(memo = "점심")))
    }

    @Test
    fun incomeCategoryDropsBenefits() {
        val edit = TxEditRules.of(tx).copy(categoryId = 11)
        assertNull(TxEditRules.benefitCard(tx, edit, ref))
        assertEquals(
            JsonObject(mapOf("categoryId" to JsonPrimitive(11), "benefitIds" to JsonArray(emptyList()))),
            patch(tx, edit),
        )
    }

    @Test
    fun memoOnlyEditKeepsOddExistingBenefits() {
        // 웹에서 수입 항목인데 혜택이 붙은 거래: 메모만 고치면 혜택은 그대로 둔다
        val income = tx.copy(categoryId = 11, isIncome = true)
        assertEquals(JsonObject(mapOf("memo" to JsonPrimitive("보너스"))), patch(income, TxEditRules.of(income).copy(memo = "보너스")))
    }

    @Test
    fun benefitsOnlyForCardsWithBenefits() {
        assertEquals(card, TxEditRules.benefitCard(tx, TxEditRules.of(tx), ref))
        val cash = tx.copy(paymentMethodId = 7, benefitIds = emptyList())
        assertNull(TxEditRules.benefitCard(cash, TxEditRules.of(cash), ref))
        // 다른 카드의 혜택 id는 버린다
        assertEquals(JsonArray(listOf(JsonPrimitive(8))), patch(tx, TxEditRules.of(tx).copy(benefitIds = listOf(8, 99)))["benefitIds"])
    }

    @Test
    fun transferEditsMemoOnly() {
        val transfer = Tx(id = 200, ledgerId = 1, date = "2026-11-21", amount = 50_000, description = "장부 이체", transferLedgerId = 2)
        assertTrue(TxEditRules.categoryChoices(transfer, ref).isEmpty())
        assertNull(TxEditRules.benefitCard(transfer, TxEditRules.of(transfer), ref))
        val edit = TxEditRules.of(transfer).copy(categoryId = 10, memo = "용돈")
        assertEquals(JsonObject(mapOf("memo" to JsonPrimitive("용돈"))), patch(transfer, edit))
    }
}
