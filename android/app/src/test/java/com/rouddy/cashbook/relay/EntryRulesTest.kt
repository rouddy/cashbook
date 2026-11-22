package com.rouddy.cashbook.relay

import com.rouddy.cashbook.relay.book.Benefit
import com.rouddy.cashbook.relay.book.Category
import com.rouddy.cashbook.relay.book.EntryKind
import com.rouddy.cashbook.relay.book.EntryRules
import com.rouddy.cashbook.relay.book.Ledger
import com.rouddy.cashbook.relay.book.PaymentMethod
import com.rouddy.cashbook.relay.book.RefData
import com.rouddy.cashbook.relay.book.Suggestion
import com.rouddy.cashbook.relay.book.Tx
import com.rouddy.cashbook.relay.book.compactWon
import com.rouddy.cashbook.relay.book.transferLabel
import com.rouddy.cashbook.relay.book.txAmountText
import com.rouddy.cashbook.relay.ui.niceStep
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate

class EntryRulesTest {
    private val coffee = Benefit(7, "커피 10%")
    private val movie = Benefit(8, "영화 5천원")
    private val points = Benefit(9, "기본 적립", kind = "POINT", stackable = true)
    private val ref = RefData(
        ledgers = listOf(Ledger(1, "가계부", isDefault = true), Ledger(2, "용돈")),
        categories = listOf(
            Category(10, 1, "외식"),
            Category(11, 1, "월급", kind = "INCOME"),
            Category(20, 2, "외식"),
        ),
        paymentMethods = listOf(
            PaymentMethod(5, "신한 딥드림", kind = "CREDIT_CARD", isCard = true, benefits = listOf(coffee, movie, points)),
            PaymentMethod(6, "국민 체크", kind = "CHECK_CARD", isCard = true),
            PaymentMethod(7, "현금", kind = "CASH"),
        ),
    )
    private val today = LocalDate.of(2026, 11, 21)
    private val blank = EntryRules.newForm(ref, today)

    @Test
    fun newFormUsesDefaultLedgerAndOtherLedgerForTransfer() {
        assertEquals(1, blank.ledgerId)
        assertEquals(2, blank.toLedgerId)
        assertEquals(EntryKind.EXPENSE, blank.kind)
    }

    @Test
    fun expenseInputWithRefundInstallmentAndBenefits() {
        val form = blank.copy(
            amount = "12,500",
            refund = true,
            description = " 스타벅스 ",
            categoryId = 10,
            paymentMethodId = 5,
            installmentMonths = 3,
            benefitIds = listOf(7, 99),
            memo = "  ",
        )
        val input = EntryRules.toInput(form, ref).getOrThrow()
        assertEquals(JsonPrimitive(-12500L), input["amount"])
        assertEquals(JsonPrimitive("스타벅스"), input["description"])
        assertEquals(JsonPrimitive(3), input["installmentMonths"])
        // 다른 카드의 혜택 id는 보내지 않는다
        assertEquals(JsonArray(listOf(JsonPrimitive(7))), input["benefitIds"])
        assertEquals(JsonNull, input["memo"])
        assertEquals(JsonPrimitive("CONFIRMED"), input["status"])
        assertNull(input["transferLedgerId"])
    }

    @Test
    fun incomeIsPositiveWithoutCardOptions() {
        val form = EntryRules.withKind(blank.copy(amount = "3000000", description = "월급", paymentMethodId = 5, benefitIds = listOf(7)), EntryKind.INCOME, ref)
            .copy(categoryId = 11, refund = true, planned = true)
        val input = EntryRules.toInput(form, ref).getOrThrow()
        assertEquals(JsonPrimitive(3000000L), input["amount"])
        assertEquals(JsonArray(emptyList()), input["benefitIds"])
        assertEquals(JsonPrimitive(1), input["installmentMonths"])
        assertEquals(JsonPrimitive("PLANNED"), input["status"])
    }

    @Test
    fun transferHasNoCategoryOrPaymentMethod() {
        val form = EntryRules.withKind(blank.copy(amount = "50000", categoryId = 10, paymentMethodId = 5), EntryKind.TRANSFER, ref)
        assertNull(form.categoryId)
        assertNull(form.paymentMethodId)
        val input = EntryRules.toInput(form, ref).getOrThrow()
        assertEquals(JsonPrimitive(1), input["ledgerId"])
        assertEquals(JsonPrimitive(2), input["transferLedgerId"])
        assertEquals(JsonPrimitive(EntryRules.TRANSFER_DEFAULT_DESCRIPTION), input["description"])
        assertNull(input["categoryId"])
        assertNull(input["paymentMethodId"])
        // 같은 장부로는 못 보낸다
        assertTrue(EntryRules.toInput(form.copy(toLedgerId = 1), ref).isFailure)
    }

    @Test
    fun validationMessages() {
        assertEquals("금액을 입력하세요", EntryRules.toInput(blank.copy(description = "x"), ref).exceptionOrNull()?.message)
        assertEquals("금액을 입력하세요", EntryRules.toInput(blank.copy(amount = "0", description = "x"), ref).exceptionOrNull()?.message)
        assertEquals("내용을 입력하세요", EntryRules.toInput(blank.copy(amount = "1000", description = " "), ref).exceptionOrNull()?.message)
        // 다른 장부의 카테고리
        assertTrue(EntryRules.toInput(blank.copy(amount = "1000", description = "x", categoryId = 20), ref).isFailure)
        // 수입 카테고리를 지출로
        assertTrue(EntryRules.toInput(blank.copy(amount = "1000", description = "x", categoryId = 11), ref).isFailure)
        assertNull(EntryRules.parseAmount("abc"))
        assertEquals(1234567L, EntryRules.parseAmount("1,234,567"))
    }

    @Test
    fun changingLedgerAndPaymentMethodDropsInvalidValues() {
        val f = EntryRules.withLedger(blank.copy(categoryId = 10), 2, ref)
        assertNull(f.categoryId)
        assertEquals(1, f.toLedgerId) // 받는 장부가 보내는 장부와 같아지면 다른 장부로
        val card = blank.copy(paymentMethodId = 5, installmentMonths = 6, benefitIds = listOf(7, 9), excludeFromPerformance = true)
        val check = EntryRules.withPaymentMethod(card, 6, ref)
        assertEquals(1, check.installmentMonths)
        assertEquals(emptyList<Int>(), check.benefitIds)
        assertTrue(check.excludeFromPerformance) // 체크카드도 실적이 있다
        assertEquals(false, EntryRules.withPaymentMethod(card, 7, ref).excludeFromPerformance)
    }

    @Test
    fun benefitStackingMatchesWeb() {
        val all = listOf(coffee, movie, points)
        var sel = EntryRules.toggleBenefit(emptyList(), coffee, true, all)
        sel = EntryRules.toggleBenefit(sel, points, true, all)
        assertEquals(listOf(7, 9), sel)
        // 중복 적용이 아닌 혜택은 하나만: 영화를 고르면 커피가 빠진다
        sel = EntryRules.toggleBenefit(sel, movie, true, all)
        assertEquals(listOf(9, 8), sel)
        assertEquals(listOf(8), EntryRules.toggleBenefit(sel, points, false, all))
    }

    @Test
    fun suggestionFillsClassificationAndSwitchesKind() {
        val s = Suggestion("월급", ledgerId = 1, categoryId = 11, paymentMethodId = 7, lastAmount = 3_000_000)
        val f = EntryRules.applySuggestion(blank.copy(description = "월"), s, ref)
        assertEquals(EntryKind.INCOME, f.kind)
        assertEquals(11, f.categoryId)
        assertEquals("3000000", f.amount)
        assertEquals(7, f.paymentMethodId)
        // 이미 고른 결제수단·금액은 그대로
        val g = EntryRules.applySuggestion(blank.copy(amount = "4500", paymentMethodId = 5), Suggestion("스타벅스", 1, 10, 6, benefitIds = listOf(1)), ref)
        assertEquals(EntryKind.EXPENSE, g.kind)
        assertEquals(5, g.paymentMethodId)
        assertEquals("4500", g.amount)
        assertEquals(emptyList<Int>(), g.benefitIds)
    }

    @Test
    fun afterSaveKeepsContext() {
        val f = blank.copy(kind = EntryKind.EXPENSE, amount = "1", description = "x", paymentMethodId = 5, planned = true, memo = "m")
        val next = EntryRules.afterSave(f)
        assertEquals(5, next.paymentMethodId)
        assertTrue(next.planned)
        assertEquals("", next.amount)
        assertEquals("", next.memo)
    }

    @Test
    fun formatting() {
        val transfer = Tx(id = 1, ledgerId = 1, date = "2026-11-21", amount = 50000, transferLedgerId = 2)
        assertEquals("→ 용돈", transferLabel(transfer, ref, 1))
        assertEquals("← 가계부", transferLabel(transfer, ref, 2))
        assertEquals("가계부 → 용돈", transferLabel(transfer, ref, null))
        assertEquals("+50,000원", txAmountText(transfer, 2))
        assertEquals("50,000원", txAmountText(transfer, null))
        assertEquals("-3,000원", txAmountText(Tx(id = 2, ledgerId = 1, date = "2026-11-21", amount = -3000), null))
        assertEquals("50만", compactWon(500_000))
        assertEquals("1.2억", compactWon(120_000_000))
        assertEquals("9,999", compactWon(9_999))
        assertEquals(250_000.0, niceStep(210_000.0), 0.0)
        assertEquals(1.0, niceStep(0.0), 0.0)
    }
}
