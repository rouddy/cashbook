package com.rouddy.cashbook.relay

import com.rouddy.cashbook.relay.parse.FormatException
import com.rouddy.cashbook.relay.parse.MessageFormat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.LocalDate

class MessageFormatTest {
    private val today = LocalDate.of(2026, 10, 23)

    // 서버 domain/sms.rs 테스트와 같은 승인 문자
    private val shinhan = "[Web발신]\n신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원"
    private val kb = "[Web발신]\nKB국민카드1234승인\n홍*동님\n1,200,000원 03개월\n10/22 15:00\n애플스토어 가로수길\n누적1,500,000원"

    @Test
    fun shinhanSingleLine() {
        val f = MessageFormat.compileTemplate("{카드}({끝자리})승인 {*} {금액}원({할부}){날짜} {시각} {가맹점} 누적{*}")
        val p = f.extract(shinhan, today)!!
        assertEquals(12_500L, p.amount)
        assertEquals("스타벅스", p.merchant)
        assertEquals(LocalDate.of(2026, 10, 23), p.date)
        assertEquals("12:34", p.time)
        assertEquals(1, p.installmentMonths)
        assertEquals("신한카드", p.card)
        assertEquals("1234", p.last4)
    }

    @Test
    fun templateFromPastedSampleWithWebPrefixAndNewlines() {
        // 샘플을 그대로 붙여 넣고 바뀌는 부분만 바꾼 형식 ([Web발신]·줄바꿈 포함)
        val template = "[Web발신]\nKB국민카드{끝자리}승인\n{*}\n{금액}원 {할부}\n{날짜} {시각}\n{가맹점}\n누적{*}"
        val p = MessageFormat.compileTemplate(template).extract(kb, today)!!
        assertEquals(1_200_000L, p.amount)
        assertEquals(3, p.installmentMonths)
        assertEquals("애플스토어 가로수길", p.merchant)
        assertEquals(LocalDate.of(2026, 10, 22), p.date)
        assertEquals("1234", p.last4)
    }

    @Test
    fun whitespaceIsOptionalAroundItems() {
        val f = MessageFormat.compileTemplate("승인 {금액}원 {가맹점} 누적")
        val p = f.extract("승인12,000 원\n  편의점누적 5,000원", today)!!
        assertEquals(12_000L, p.amount)
        assertEquals("편의점", p.merchant)
    }

    @Test
    fun lastItemTakesRestOfLine() {
        val p = MessageFormat.compileTemplate("{금액}원 결제 {가맹점}").extract("8,900원 결제 GS25 역삼점\n다음 줄", today)!!
        assertEquals("GS25 역삼점", p.merchant)
    }

    @Test
    fun mismatchReturnsNull() {
        val f = MessageFormat.compileTemplate("{카드}({끝자리})승인 {*} {금액}원")
        assertNull(f.extract("[광고] 신한카드 이벤트 안내 5,000원 할인", today))
    }

    @Test
    fun notificationStyleTitleAndBody() {
        // 카카오페이 같은 앱 알림: "제목\n본문"
        val f = MessageFormat.compileTemplate("{가맹점}\n{금액}원 결제 완료")
        val p = f.extract("스타벅스 강남점\n4,500원 결제 완료", today)!!
        assertEquals("스타벅스 강남점", p.merchant)
        assertEquals(4_500L, p.amount)
    }

    @Test
    fun koreanDateAndFullDate() {
        val f = MessageFormat.compileTemplate("{날짜} {금액}원")
        assertEquals(LocalDate.of(2026, 10, 3), f.extract("10월 3일 1,000원", today)!!.date)
        assertEquals(LocalDate.of(2025, 12, 31), f.extract("2025.12.31 1,000원", today)!!.date)
        assertEquals(LocalDate.of(2025, 12, 31), f.extract("25-12-31 1,000원", today)!!.date)
    }

    @Test
    fun yearRolloverLikeServer() {
        assertEquals(LocalDate.of(2026, 12, 31), MessageFormat.parseDate("12/31", LocalDate.of(2027, 1, 2)))
        assertEquals(LocalDate.of(2026, 10, 30), MessageFormat.parseDate("10/30", today)) // 7일 안의 미래는 올해
        assertNull(MessageFormat.parseDate("13/45", today))
    }

    @Test
    fun maskedLast4IsDropped() {
        val p = MessageFormat.compileTemplate("하나({끝자리})승인 {*} {금액}원").extract("하나(1*2*)승인 홍*동님 8,000원", today)!!
        assertNull(p.last4)
        assertEquals(8_000L, p.amount)
    }

    @Test
    fun regexMode() {
        val f = MessageFormat.compileRegex("""(?<amount>[\d,]+)원.*?(?<date>\d{2}/\d{2})\s+\d{2}:\d{2}\s+(?<merchant>\S+)""")
        val p = f.extract(shinhan, today)!!
        assertEquals(12_500L, p.amount)
        assertEquals(LocalDate.of(2026, 10, 23), p.date)
        assertEquals("스타벅스", p.merchant)
        assertNull(p.time)
    }

    @Test
    fun templateErrors() {
        assertThrows(FormatException::class.java) { MessageFormat.compileTemplate("") }
        assertThrows(FormatException::class.java) { MessageFormat.compileTemplate("승인 문자") }
        val unknown = assertThrows(FormatException::class.java) { MessageFormat.compileTemplate("{가격}원") }
        assertTrue(unknown.message!!.contains("{가격}"))
        assertThrows(FormatException::class.java) { MessageFormat.compileTemplate("{금액}원 누적 {금액}원") }
        assertThrows(FormatException::class.java) { MessageFormat.compileRegex("(?<amount>[") }
        assertThrows(FormatException::class.java) { MessageFormat.compileRegex("""(\d+)원""") }
    }

    @Test
    fun regexSpecialCharactersInTemplateAreLiteral() {
        val f = MessageFormat.compileTemplate("[승인] {금액}원 (*{가맹점}*)")
        val p = f.extract("[승인] 3,000원 (*김밥천국*)", today)
        assertNotNull(p)
        assertEquals("김밥천국", p!!.merchant)
    }

    @Test
    fun installmentAndTime() {
        assertEquals(1, MessageFormat.parseInstallment("일시불"))
        assertEquals(12, MessageFormat.parseInstallment("12 개월"))
        assertEquals("09:05", MessageFormat.parseTime("9:05:33"))
        assertNull(MessageFormat.parseTime("25:00"))
    }
}
