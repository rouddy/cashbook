package com.rouddy.cashbook.relay.parse

/**
 * 발신 번호 비교용 정리: 숫자만 남기고 국제 형식(+82)은 국내 형식으로.
 * 숫자가 없는 발신자(영문 이름 등)는 소문자 그대로.
 */
object PhoneNumber {
    fun normalize(raw: String): String {
        val trimmed = raw.trim()
        val digits = trimmed.filter(Char::isDigit)
        if (digits.isEmpty()) return trimmed.lowercase()
        if (!trimmed.startsWith("+82")) return digits
        val national = digits.removePrefix("82")
        // 15xx·16xx·18xx 대표번호(8자리)는 앞에 0이 없다. 나머지(+82 10…, +82 2…)는 0을 붙인다.
        return if (national.length == 8 && national.startsWith("1")) national else "0$national"
    }

    fun same(a: String, b: String): Boolean {
        val na = normalize(a)
        return na.isNotEmpty() && na == normalize(b)
    }

    /** 보기 좋게: 01012345678 → 010-1234-5678, 15447200 → 1544-7200 */
    fun format(raw: String): String {
        val d = normalize(raw)
        if (!d.all(Char::isDigit)) return raw.trim()
        return when {
            d.length == 8 && (d.startsWith("15") || d.startsWith("16") || d.startsWith("18")) -> "${d.take(4)}-${d.drop(4)}"
            d.length == 11 && d.startsWith("01") -> "${d.take(3)}-${d.substring(3, 7)}-${d.drop(7)}"
            d.length == 10 && d.startsWith("02") -> "${d.take(2)}-${d.substring(2, 6)}-${d.drop(6)}"
            d.length == 9 && d.startsWith("02") -> "${d.take(2)}-${d.substring(2, 5)}-${d.drop(5)}"
            d.length == 10 && d.startsWith("0") -> "${d.take(3)}-${d.substring(3, 6)}-${d.drop(6)}"
            else -> d
        }
    }
}
