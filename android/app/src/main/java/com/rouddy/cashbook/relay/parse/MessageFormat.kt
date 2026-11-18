package com.rouddy.cashbook.relay.parse

import java.time.LocalDate
import java.util.regex.Pattern
import java.util.regex.PatternSyntaxException

/**
 * 문자/알림 형식에서 뽑을 수 있는 항목.
 * [token]은 템플릿의 `{금액}` 같은 이름, [group]은 정규식의 이름 있는 그룹 이름이다.
 */
enum class Field(val token: String, val group: String, val aliases: List<String>, val hint: String) {
    AMOUNT("금액", "amount", listOf("amount"), "12,500"),
    MERCHANT("가맹점", "merchant", listOf("내용", "merchant"), "가맹점 이름"),
    DATE("날짜", "date", listOf("date"), "10/23, 2026.10.23, 10월 23일"),
    TIME("시각", "time", listOf("시간", "time"), "12:34"),
    INSTALLMENT("할부", "installment", listOf("installment"), "일시불, 03개월"),
    CARD("카드", "card", listOf("card"), "카드 이름"),
    LAST4("끝자리", "last4", listOf("last4"), "카드번호 끝 4자리");

    val placeholder: String get() = "{$token}"

    companion object {
        fun byName(name: String): Field? =
            entries.firstOrNull { it.token == name || name in it.aliases || it.group.equals(name, ignoreCase = true) }
    }
}

/** 아무 글자나 건너뛰는 템플릿 항목 */
const val WILDCARD = "{*}"

/** 형식에서 뽑은 값. 형식에 없는 항목은 null (서버가 원문에서 다시 찾는다). */
data class ParsedFields(
    val amount: Long? = null,
    val merchant: String? = null,
    val date: LocalDate? = null,
    /** "HH:mm" */
    val time: String? = null,
    val installmentMonths: Int? = null,
    val card: String? = null,
    val last4: String? = null,
)

class FormatException(message: String) : IllegalArgumentException(message)

/** 컴파일한 형식. [groups]는 정규식에 실제로 있는 항목 그룹 이름. */
class CompiledFormat internal constructor(val pattern: Pattern, val groups: Set<String>) {

    /** 형식과 맞으면 뽑은 값, 아니면 null */
    fun extract(text: String, today: LocalDate): ParsedFields? {
        val m = pattern.matcher(MessageFormat.normalize(text))
        if (!m.find()) return null
        fun g(field: Field): String? =
            if (field.group in groups) m.group(field.group)?.trim()?.takeIf { it.isNotEmpty() } else null
        return ParsedFields(
            amount = g(Field.AMOUNT)?.filter(Char::isDigit)?.toLongOrNull()?.takeIf { it > 0 },
            merchant = g(Field.MERCHANT)?.take(100),
            date = g(Field.DATE)?.let { MessageFormat.parseDate(it, today) },
            time = g(Field.TIME)?.let(MessageFormat::parseTime),
            installmentMonths = g(Field.INSTALLMENT)?.let(MessageFormat::parseInstallment),
            card = g(Field.CARD),
            last4 = g(Field.LAST4)?.takeIf { it.length == 4 && it.all(Char::isDigit) },
        )
    }
}

/**
 * 템플릿 형식: 샘플 문자에서 바뀌는 부분을 `{금액}`, `{가맹점}` 같은 항목으로 바꾼 것.
 * - 항목 밖의 글자는 그대로 맞춰 본다. 공백·줄바꿈은 있어도 없어도 맞는다(항목 앞뒤 포함).
 * - `{*}`는 아무 글자나(줄바꿈 포함) 건너뛴다.
 * - 문자 전체가 아니라 일부만 맞아도 된다. `[Web발신]`은 무시한다.
 * - `{가맹점}`·`{카드}`는 한 줄 안에서만 찾고, 마지막 항목(또는 `{*}` 바로 앞)이면 줄 끝까지 가져간다.
 */
object MessageFormat {
    private val TOKEN = Regex("""\{([^{}]*)\}""")
    private val WHITESPACE = Regex("""\s+""")
    private val GROUP_NAME = Regex("""\(\?<([a-zA-Z][a-zA-Z0-9]*)>""")
    private val DIGITS = Regex("""\d+""")
    private const val GAP = """\s*"""

    fun normalize(text: String): String = text.replace("[Web발신]", "\n").replace("\r", "")

    private sealed interface Token {
        data class Literal(val text: String) : Token
        data class Item(val field: Field) : Token
        data object Skip : Token
    }

    fun compileTemplate(template: String): CompiledFormat {
        val source = normalize(template).trim()
        if (source.isEmpty()) throw FormatException("형식을 입력하세요")
        val tokens = mutableListOf<Token>()
        var last = 0
        for (m in TOKEN.findAll(source)) {
            if (m.range.first > last) tokens += Token.Literal(source.substring(last, m.range.first))
            val name = m.groupValues[1].trim()
            tokens += if (name == "*") {
                Token.Skip
            } else {
                Token.Item(
                    Field.byName(name) ?: throw FormatException(
                        "알 수 없는 항목 {$name} — 쓸 수 있는 항목: " +
                            (Field.entries.map { it.placeholder } + WILDCARD).joinToString(" "),
                    ),
                )
            }
            last = m.range.last + 1
        }
        if (last < source.length) tokens += Token.Literal(source.substring(last))

        val items = tokens.filterIsInstance<Token.Item>().map { it.field }
        if (items.isEmpty()) throw FormatException("{금액}, {가맹점} 같은 항목을 하나 이상 넣으세요")
        items.groupBy { it }.filterValues { it.size > 1 }.keys.firstOrNull()?.let {
            throw FormatException("${it.placeholder}는 한 번만 쓸 수 있습니다. 나머지는 {*}로 건너뛰세요")
        }

        val regex = tokens.mapIndexed { i, token ->
            when (token) {
                is Token.Literal -> literal(token.text)
                Token.Skip -> """[\s\S]*?"""
                is Token.Item -> {
                    val next = tokens.drop(i + 1).firstOrNull { !(it is Token.Literal && it.text.isBlank()) }
                    "(?<${token.field.group}>${pattern(token.field, open = next == null || next == Token.Skip)})"
                }
            }
        }.filter { it.isNotEmpty() }.joinToString(GAP)
        return try {
            CompiledFormat(Pattern.compile(regex), items.map { it.group }.toSet())
        } catch (e: PatternSyntaxException) {
            throw FormatException("형식을 해석하지 못했습니다: ${e.description}")
        }
    }

    /** 정규식 형식: 이름 있는 그룹 (?<amount>…), merchant, date, time, installment, card, last4 */
    fun compileRegex(regex: String): CompiledFormat {
        if (regex.isBlank()) throw FormatException("정규식을 입력하세요")
        val pattern = try {
            Pattern.compile(regex.trim())
        } catch (e: PatternSyntaxException) {
            throw FormatException("정규식 오류: ${e.description} (위치 ${e.index})")
        }
        val known = Field.entries.map { it.group }.toSet()
        val groups = GROUP_NAME.findAll(regex).map { it.groupValues[1] }.filter { it in known }.toSet()
        if (groups.isEmpty()) {
            throw FormatException("이름 있는 그룹이 하나 이상 필요합니다. 예: (?<amount>[\\d,]+)원 — 이름: ${known.joinToString(", ")}")
        }
        return CompiledFormat(pattern, groups)
    }

    private fun literal(text: String): String {
        val out = StringBuilder()
        var i = 0
        for (ws in WHITESPACE.findAll(text)) {
            if (ws.range.first > i) out.append(Pattern.quote(text.substring(i, ws.range.first)))
            out.append(GAP)
            i = ws.range.last + 1
        }
        if (i < text.length) out.append(Pattern.quote(text.substring(i)))
        return out.toString()
    }

    private fun pattern(field: Field, open: Boolean): String = when (field) {
        Field.AMOUNT -> """\d{1,3}(?:,\d{3})+|\d+"""
        Field.MERCHANT, Field.CARD -> if (open) """[^\n]+""" else """[^\n]+?"""
        Field.DATE -> """\d{4}[./-]\d{1,2}[./-]\d{1,2}|\d{2}[./-]\d{1,2}[./-]\d{1,2}|\d{1,2}[./-]\d{1,2}|\d{1,2}월\s*\d{1,2}일"""
        Field.TIME -> """\d{1,2}:\d{2}(?::\d{2})?"""
        Field.INSTALLMENT -> """일시불|\d{1,2}\s*개월"""
        Field.LAST4 -> """[\d*]{4}"""
    }

    /** "10/23"처럼 연도가 없으면 올해로 보되, 일주일 넘게 미래면 작년 (서버 `sms::infer_date`와 같은 규칙) */
    fun parseDate(raw: String, today: LocalDate): LocalDate? {
        val n = DIGITS.findAll(raw).map { it.value }.toList()
        return runCatching {
            when (n.size) {
                3 -> {
                    val year = n[0].toInt().let { if (n[0].length <= 2) 2000 + it else it }
                    LocalDate.of(year, n[1].toInt(), n[2].toInt())
                }
                2 -> {
                    val date = LocalDate.of(today.year, n[0].toInt(), n[1].toInt())
                    if (date.isAfter(today.plusDays(7))) date.minusYears(1) else date
                }
                else -> null
            }
        }.getOrNull()
    }

    fun parseTime(raw: String): String? {
        val parts = raw.split(':')
        val h = parts.getOrNull(0)?.toIntOrNull() ?: return null
        val m = parts.getOrNull(1)?.toIntOrNull() ?: return null
        if (h !in 0..23 || m !in 0..59) return null
        return "%02d:%02d".format(h, m)
    }

    fun parseInstallment(raw: String): Int? =
        if (raw.contains("일시불")) 1 else DIGITS.find(raw)?.value?.toIntOrNull()?.takeIf { it in 1..60 }
}
