package com.rouddy.cashbook.relay

import com.rouddy.cashbook.relay.data.ParseMode
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.ingest.Decision
import com.rouddy.cashbook.relay.ingest.IncomingMessage
import com.rouddy.cashbook.relay.ingest.RuleMatcher
import com.rouddy.cashbook.relay.parse.PhoneNumber
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate

class RuleMatcherTest {
    private val today = LocalDate.of(2026, 10, 23)
    private val text = "[Web발신]\n신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원"
    private val sms = IncomingMessage(SourceType.SMS, "+8215447200", body = text)

    private val shinhan = Rule(
        name = "신한",
        source = SourceType.SMS,
        senders = listOf("1544-7200"),
        format = "{카드}({끝자리})승인 {*} {금액}원({할부}){날짜} {시각} {가맹점} 누적{*}",
    )

    @Test
    fun phoneNumbers() {
        assertEquals("15447200", PhoneNumber.normalize("+8215447200"))
        assertEquals("01012345678", PhoneNumber.normalize("+82 10-1234-5678"))
        assertEquals("0212345678", PhoneNumber.normalize("+82-2-1234-5678"))
        assertTrue(PhoneNumber.same("1544-7200", "15447200"))
        assertFalse(PhoneNumber.same("", ""))
        assertEquals("1544-7200", PhoneNumber.format("15447200"))
        assertEquals("010-1234-5678", PhoneNumber.format("+821012345678"))
    }

    @Test
    fun matchingRuleBuildsPayload() {
        val d = RuleMatcher.decide(listOf(shinhan), sms, today) as Decision.Upload
        val p = d.payload
        assertEquals(text, p["text"]!!.jsonPrimitive.content)
        assertEquals(12_500L, p["amount"]!!.jsonPrimitive.content.toLong())
        assertEquals("스타벅스", p["merchant"]!!.jsonPrimitive.content)
        assertEquals("2026-10-23", p["date"]!!.jsonPrimitive.content)
        assertEquals(1, p["installmentMonths"]!!.jsonPrimitive.content.toInt())
        // 규칙에 결제수단이 없으면 끝자리 → 카드 이름 순
        assertEquals("1234", p["card"]!!.jsonPrimitive.content)
        assertNull(p["cancel"])
    }

    @Test
    fun ruleCardOverridesParsedCard() {
        val d = RuleMatcher.decide(listOf(shinhan.copy(card = "MG+S", ledger = "용돈", category = "외식")), sms, today) as Decision.Upload
        assertEquals("MG+S", d.payload["card"]!!.jsonPrimitive.content)
        assertEquals("용돈", d.payload["ledger"]!!.jsonPrimitive.content)
        assertEquals("외식", d.payload["category"]!!.jsonPrimitive.content)
    }

    @Test
    fun otherSenderOrDisabledIsIgnored() {
        assertEquals(Decision.Ignore, RuleMatcher.decide(listOf(shinhan), sms.copy(origin = "01099998888"), today))
        assertEquals(Decision.Ignore, RuleMatcher.decide(listOf(shinhan.copy(enabled = false)), sms, today))
        // 번호를 비우면 모든 번호
        assertTrue(RuleMatcher.decide(listOf(shinhan.copy(senders = emptyList())), sms.copy(origin = "01099998888"), today) is Decision.Upload)
    }

    @Test
    fun unmatchedFormatIsSkippedUnlessRawFallback() {
        val ad = sms.copy(body = "[Web발신]\n(광고) 신한카드 이벤트 5,000원 할인")
        assertTrue(RuleMatcher.decide(listOf(shinhan), ad, today) is Decision.Unmatched)
        val raw = RuleMatcher.decide(listOf(shinhan.copy(sendRawIfUnmatched = true)), ad, today) as Decision.Upload
        assertEquals(setOf("text"), raw.payload.keys)
    }

    @Test
    fun firstMatchingRuleWinsAndCancelRule() {
        val cancelRule = shinhan.copy(id = "cancel", name = "신한 취소", format = "{카드}({끝자리})승인취소 {*} {금액}원", cancel = true)
        val cancelSms = sms.copy(body = "[Web발신]\n신한카드(1234)승인취소 홍*동 12,500원(일시불)10/23 12:40 스타벅스")
        val d = RuleMatcher.decide(listOf(cancelRule, shinhan), cancelSms, today) as Decision.Upload
        assertEquals("cancel", d.rule.id)
        assertEquals(JsonPrimitive(true), d.payload["cancel"])
        // 승인 문자는 취소 규칙 형식과 맞지 않으므로 다음 규칙으로
        assertEquals(shinhan.id, (RuleMatcher.decide(listOf(cancelRule, shinhan), sms, today) as Decision.Upload).rule.id)
    }

    @Test
    fun keywordsFilter() {
        val rule = shinhan.copy(parseMode = ParseMode.SERVER, keywords = "승인, 결제")
        assertTrue(RuleMatcher.decide(listOf(rule), sms, today) is Decision.Upload)
        assertEquals(Decision.Ignore, RuleMatcher.decide(listOf(rule), sms.copy(body = "이번 달 청구 금액 안내 300,000원"), today))
    }

    @Test
    fun notificationRuleMatchesPackageAndTitle() {
        val rule = Rule(
            source = SourceType.NOTIFICATION,
            packageName = "com.kakao.talk",
            appLabel = "카카오톡",
            titleContains = "신한카드",
            parseMode = ParseMode.SERVER,
        )
        val n = IncomingMessage(SourceType.NOTIFICATION, "com.kakao.talk", "신한카드", "승인 12,500원 스타벅스")
        val d = RuleMatcher.decide(listOf(rule), n, today) as Decision.Upload
        assertEquals("신한카드\n승인 12,500원 스타벅스", d.payload["text"]!!.jsonPrimitive.content)
        assertEquals(Decision.Ignore, RuleMatcher.decide(listOf(rule), n.copy(title = "엄마"), today))
        assertEquals(Decision.Ignore, RuleMatcher.decide(listOf(rule), n.copy(origin = "com.other"), today))
        // SMS 규칙은 알림에 쓰지 않는다
        assertEquals(Decision.Ignore, RuleMatcher.decide(listOf(shinhan.copy(senders = emptyList())), n, today))
    }

    @Test
    fun dedupeKeyIgnoresWhitespaceButNotOrigin() {
        assertEquals(sms.dedupeKey, sms.copy(body = text.replace(" ", "  ")).dedupeKey)
        assertEquals(sms.dedupeKey, sms.copy(origin = "1544-7200").dedupeKey)
        assertNotEquals(sms.dedupeKey, sms.copy(origin = "15881688").dedupeKey)
    }

    @Test
    fun testPreview() {
        val ok = RuleMatcher.test(shinhan, text, today)
        assertNull(ok.formatError)
        assertEquals(12_500L, ok.parsed!!.amount)
        val bad = RuleMatcher.test(shinhan.copy(format = "{없음}"), text, today)
        assertTrue(bad.formatError!!.contains("{없음}"))
        val miss = RuleMatcher.test(shinhan, "다른 문자", today)
        assertNull(miss.parsed)
        assertNull(miss.payload)
    }
}
