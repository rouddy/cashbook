package com.rouddy.cashbook.relay

import com.rouddy.cashbook.relay.data.Store
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.data.UploadRecord
import com.rouddy.cashbook.relay.data.UploadStatus
import com.rouddy.cashbook.relay.ingest.CashbookApi
import com.rouddy.cashbook.relay.ingest.CashbookApi.Outcome
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CashbookApiTest {
    // routes.rs의 outcome_json 형태
    private val okBody = """
        {"ok":true,"created":true,"duplicate":false,"matchedPlanned":true,"warnings":["결제수단 'x'에 해당하는 카드를 찾지 못했습니다"],
         "parsed":{"amount":12500},
         "transaction":{"id":8612,"date":"2026-10-23","amount":12500,"description":"스타벅스","ledger":"가계부",
                        "category":"외식","paymentMethod":null,"status":"CONFIRMED","installmentMonths":1}}
    """.trimIndent()

    @Test
    fun classifyStatuses() {
        assertTrue(CashbookApi.classify(200, okBody) is Outcome.Ok)
        assertTrue(CashbookApi.classify(200, "<!doctype html>") is Outcome.Rejected)
        assertEquals(401, (CashbookApi.classify(401, """{"ok":false,"error":"x"}""") as Outcome.Rejected).status)
        assertEquals("금액이 0원입니다", (CashbookApi.classify(422, """{"ok":false,"error":"금액이 0원입니다"}""") as Outcome.Rejected).message)
        assertTrue(CashbookApi.classify(502, "Bad Gateway") is Outcome.Retry)
        assertTrue(CashbookApi.classify(429, "") is Outcome.Retry)
        assertTrue(CashbookApi.classify(301, "") is Outcome.Rejected)
    }

    @Test
    fun parseResult() {
        val body = (CashbookApi.classify(200, okBody) as Outcome.Ok).body
        val (duplicate, r) = CashbookApi.parseResult(body)
        assertFalse(duplicate)
        assertEquals(8612L, r.transactionId)
        assertEquals(12_500L, r.amount)
        assertEquals("스타벅스", r.description)
        assertEquals("외식", r.category)
        assertEquals(null, r.paymentMethod)
        assertTrue(r.matchedPlanned)
        assertEquals(1, r.warnings.size)
    }

    private fun record(status: UploadStatus, i: Int) = UploadRecord(
        receivedAt = i.toLong(),
        source = SourceType.SMS,
        origin = "1",
        originLabel = "1",
        ruleId = null,
        ruleName = "",
        text = "$i",
        dedupeKey = "$i",
        payload = null,
        status = status,
    )

    @Test
    fun historyTrimKeepsPendingAndLimitsSkipped() {
        val list = (0 until 50).map { record(UploadStatus.SKIPPED, it) } +
            (0 until 400).map { record(UploadStatus.SUCCESS, 100 + it) } +
            listOf(record(UploadStatus.PENDING, 9999))
        val trimmed = Store.trim(list)
        assertEquals(Store.MAX_SKIPPED, trimmed.count { it.status == UploadStatus.SKIPPED })
        assertEquals(Store.MAX_RECORDS, trimmed.count { it.status != UploadStatus.PENDING })
        assertEquals(1, trimmed.count { it.status == UploadStatus.PENDING })
    }
}
