package com.rouddy.cashbook.relay

import com.rouddy.cashbook.relay.book.BookApi
import com.rouddy.cashbook.relay.book.BookException
import com.rouddy.cashbook.relay.book.Dashboard
import com.rouddy.cashbook.relay.book.RefData
import com.rouddy.cashbook.relay.ingest.CashbookApi
import com.rouddy.cashbook.relay.ingest.CashbookApi.Outcome
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonArray
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.time.LocalDate

class BookApiTest {
    private fun ok(body: String) = CashbookApi.classify(200, body)

    private inline fun expectError(kind: BookException.Kind, block: () -> Unit): BookException {
        try {
            block()
        } catch (e: BookException) {
            assertEquals(kind, e.kind)
            return e
        }
        fail("BookException($kind) 예상")
        throw AssertionError()
    }

    @Test
    fun parsesRefData() {
        // Queries.REF 응답 형태
        val ref = BookApi.interpret(
            ok(
                """{"data":{"ledgers":[{"id":1,"name":"가계부","isDefault":true,"sortOrder":0},{"id":2,"name":"용돈","isDefault":false,"sortOrder":1}],
                "categories":[{"id":10,"ledgerId":1,"name":"외식","kind":"EXPENSE","archived":false},
                              {"id":11,"ledgerId":1,"name":"월급","kind":"INCOME","archived":false},
                              {"id":12,"ledgerId":1,"name":"옛날","kind":"EXPENSE","archived":true}],
                "paymentMethods":[{"id":5,"name":"신한 딥드림","kind":"CREDIT_CARD","isCard":true,"archived":false,
                                   "benefits":[{"id":7,"name":"커피 10%","kind":"DISCOUNT","stackable":false}]}],
                "members":["송화"]}}""",
            ),
            RefData.serializer(),
        )
        assertEquals(1, ref.defaultLedger?.id)
        assertEquals(listOf("외식"), ref.categoriesFor(1, income = false).map { it.name })
        assertEquals(listOf("월급"), ref.categoriesFor(1, income = true).map { it.name })
        assertTrue(ref.paymentMethod(5)!!.isCreditCard)
        assertEquals("옛날", ref.category(12)?.name) // 보관한 항목도 이름은 보인다
    }

    @Test
    fun parsesDashboardWithNullsAndDecimals() {
        val d = BookApi.interpret(
            ok(
                """{"data":{
                  "monthlySummary":{"month":"2026-11","ledgerId":null,"averageMonthsUsed":6,"spending":120000,"plannedSpending":30000,
                    "income":0,"plannedIncome":0,"transferIn":0,"plannedTransferIn":0,"transferOut":0,"plannedTransferOut":0,
                    "averageSpending":98765.4321,"budgetTotal":null,"balance":null,"projectedBalance":null,
                    "categories":[{"categoryId":null,"ledgerId":null,"name":"미분류","kind":"EXPENSE","confirmed":1000,"planned":0,"count":1,"average":0.5,"budget":null}],
                    "daily":[{"day":1,"date":"2026-11-01","spending":1000,"cumulative":1000,"projectedCumulative":1000,"averageCumulative":1234.5}]},
                  "cardsMonthly":[{"paymentMethodId":5,"spending":0,"plannedSpending":0,"performance":0,"projectedPerformance":0,
                    "performanceSteps":[],"target":null,"remainingToTarget":null,"currentLimit":null,"limitUsed":0,
                    "discountTotal":0,"pointTotal":0,"mileageTotal":0,"achievedTier":null,"nextTier":null,"remainingToNextTier":null}],
                  "availableFunds":{"today":"2026-11-21","accountsTotal":0,"balanceDate":null,"cardDue":0,"cardDuePlanned":0,
                    "unreflectedOutflow":0,"plannedOutflow":0,"plannedInflow":0,"available":0,"availableAfterPlanned":0,
                    "cards":[{"paymentMethodId":5,"counted":true,"nextPayment":null}]},
                  "planned":{"totalCount":0,"items":[]}}}""",
            ),
            Dashboard.serializer(),
        )
        assertEquals(98765.4321, d.monthlySummary.averageSpending, 1e-9)
        assertNull(d.monthlySummary.balance)
        assertFalse(d.cardsMonthly[0].worthShowing)
        assertEquals(1234.5, d.monthlySummary.daily[0].averageCumulative, 1e-9)
    }

    @Test
    fun graphQlErrorsBecomeMessages() {
        // 서버 user_error 형태
        val e = expectError(BookException.Kind.SERVER) {
            BookApi.interpret(
                ok("""{"data":null,"errors":[{"message":"내용을 입력하세요","path":["createTransaction"],"extensions":{"code":"BAD_USER_INPUT"}}]}"""),
                RefData.serializer(),
            )
        }
        assertEquals("내용을 입력하세요", e.message)
        assertFalse(e.fixInSettings)
    }

    @Test
    fun oldServerTokenRejectionIsRecognized() {
        // 토큰 권한을 합치기 전 서버의 FullAccess 가드 403 (예전 자동 입력 전용 토큰)
        val e = expectError(BookException.Kind.TOKEN_NOT_ALLOWED) {
            BookApi.interpret(
                CashbookApi.classify(403, """{"ok":false,"error":"자동 입력 전용 토큰은 /api/ingest 에만 쓸 수 있습니다"}"""),
                RefData.serializer(),
            )
        }
        assertTrue(e.fixInSettings)
        expectError(BookException.Kind.SETTINGS) { BookApi.interpret(CashbookApi.classify(401, "{}"), RefData.serializer()) }
        expectError(BookException.Kind.NETWORK) { BookApi.interpret(Outcome.Retry("연결 실패"), RefData.serializer()) }
        expectError(BookException.Kind.SERVER) { BookApi.interpret(ok("""{"data":{"ledgers":"x"}}"""), RefData.serializer()) }
    }

    @Test
    fun recentFilterAndPlannedRange() {
        val all = BookApi.recentFilter(autoOnly = false)
        assertEquals(listOf(JsonPrimitive("CONFIRMED")), all["statuses"]!!.jsonArray.toList())
        assertNull(all["source"])
        assertEquals(JsonPrimitive("API"), BookApi.recentFilter(autoOnly = true)["source"])
        val (from, to) = BookApi.plannedRange(LocalDate.of(2026, 11, 21))
        assertEquals(LocalDate.of(2026, 8, 21), from)
        assertEquals(LocalDate.of(2026, 11, 28), to)
    }
}
