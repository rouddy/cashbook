package com.rouddy.cashbook.relay.ingest

import com.rouddy.cashbook.relay.data.CardMapping
import com.rouddy.cashbook.relay.data.ParseMode
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.parse.CompiledFormat
import com.rouddy.cashbook.relay.parse.Field
import com.rouddy.cashbook.relay.parse.FormatException
import com.rouddy.cashbook.relay.parse.MessageFormat
import com.rouddy.cashbook.relay.parse.ParsedFields
import com.rouddy.cashbook.relay.parse.PhoneNumber
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.security.MessageDigest
import java.time.LocalDate

/** 받은 문자/알림 하나 */
data class IncomingMessage(
    val source: SourceType,
    /** 발신 번호 또는 앱 패키지 */
    val origin: String,
    /** 알림 제목 (문자는 빈 값) */
    val title: String = "",
    val body: String,
    val receivedAt: Long = System.currentTimeMillis(),
) {
    /** 형식을 맞춰 보고 서버로 보내는 원문. 알림은 "제목\n본문" */
    val fullText: String
        get() = if (source == SourceType.NOTIFICATION && title.isNotBlank()) "$title\n$body" else body

    /** 같은 출처의 같은 원문이면 같은 키 (공백 차이 무시) */
    val dedupeKey: String
        get() {
            val normalized = fullText.split(Regex("""\s+""")).filter { it.isNotEmpty() }.joinToString(" ")
            val origin = if (source == SourceType.SMS) PhoneNumber.normalize(origin) else origin
            val digest = MessageDigest.getInstance("SHA-256").digest("$source|$origin|$normalized".toByteArray())
            return digest.joinToString("") { "%02x".format(it) }
        }
}

sealed interface Decision {
    /** 어떤 규칙에도 해당하지 않음 (대부분의 문자/알림) */
    data object Ignore : Decision

    /** 규칙 대상(번호·앱·필터는 맞음)이지만 형식이 맞지 않음 */
    data class Unmatched(val rule: Rule) : Decision

    data class Upload(val rule: Rule, val parsed: ParsedFields?, val payload: JsonObject) : Decision
}

/** 규칙 화면의 시험 결과 */
data class TestResult(
    val formatError: String?,
    val keywordMissing: Boolean,
    val titleMissing: Boolean,
    /** 형식이 맞아 뽑은 값 (서버 해석 방식이면 null) */
    val parsed: ParsedFields?,
    /** 보낼 JSON. 보내지 않으면 null */
    val payload: JsonObject?,
    /** 보낼 결제수단과 그 출처 (보내지 않거나 결제수단이 없으면 null) */
    val card: CardChoice? = null,
)

/** 서버로 보낼 결제수단(`card`)을 어디서 정했는지 */
enum class CardSource {
    /** 규칙의 카드별 결제수단 */
    MAPPING,

    /** 규칙의 (기본) 결제수단 */
    RULE,

    /** 형식의 `{끝자리}` */
    LAST4,

    /** 형식의 `{카드}` */
    CARD,
}

data class CardChoice(val value: String, val source: CardSource, val mapping: CardMapping? = null)

object RuleMatcher {

    fun compile(rule: Rule): CompiledFormat? = when (rule.parseMode) {
        ParseMode.TEMPLATE -> MessageFormat.compileTemplate(rule.format)
        ParseMode.REGEX -> MessageFormat.compileRegex(rule.format)
        ParseMode.SERVER -> null
    }

    /** 형식 오류 메시지 (없으면 null) */
    fun formatError(rule: Rule): String? = try {
        compile(rule)
        null
    } catch (e: FormatException) {
        e.message
    }

    /** 형식에 `{카드}`나 `{끝자리}`가 있는지 (카드별 결제수단을 쓸 수 있는지) */
    fun capturesCard(rule: Rule): Boolean = try {
        compile(rule)?.groups?.any { it == Field.CARD.group || it == Field.LAST4.group } == true
    } catch (_: FormatException) {
        false
    }

    private fun squash(s: String): String = s.filterNot(Char::isWhitespace).lowercase()

    /**
     * 뽑은 `{카드}`·`{끝자리}` 값에 맞는 카드별 결제수단. 여러 줄이 맞으면 더 긴 값이 맞는 줄
     * (예: "신한"과 "신한 Deep Dream"이 모두 맞으면 뒤의 것).
     */
    fun cardMapping(rule: Rule, parsed: ParsedFields?): CardMapping? {
        val values = listOfNotNull(parsed?.card, parsed?.last4).map(::squash)
        if (values.isEmpty()) return null
        return rule.cardMappings
            .filter { m -> m.card.isNotBlank() && squash(m.match).let { k -> k.isNotEmpty() && values.any { it.contains(k) } } }
            .maxByOrNull { squash(it.match).length }
    }

    /** 보낼 결제수단: 카드별 결제수단 → 규칙의 결제수단 → `{끝자리}` → `{카드}` */
    fun cardChoice(rule: Rule?, parsed: ParsedFields?): CardChoice? {
        rule?.let { cardMapping(it, parsed) }?.let { return CardChoice(it.card.trim(), CardSource.MAPPING, it) }
        rule?.card?.trim()?.takeIf(String::isNotEmpty)?.let { return CardChoice(it, CardSource.RULE) }
        parsed?.last4?.let { return CardChoice(it, CardSource.LAST4) }
        parsed?.card?.let { return CardChoice(it, CardSource.CARD) }
        return null
    }

    fun keywordList(rule: Rule): List<String> = rule.keywords.split(',').map(String::trim).filter(String::isNotEmpty)

    private fun keywordsOk(rule: Rule, text: String): Boolean {
        val keywords = keywordList(rule)
        return keywords.isEmpty() || keywords.any { text.contains(it, ignoreCase = true) }
    }

    private fun titleOk(rule: Rule, title: String): Boolean =
        rule.titleContains.isBlank() || title.contains(rule.titleContains.trim(), ignoreCase = true)

    /** 출처·번호/앱·제목·포함 단어가 맞는지 (형식은 보지 않음) */
    fun appliesTo(rule: Rule, msg: IncomingMessage): Boolean {
        if (rule.source != msg.source) return false
        val originOk = when (rule.source) {
            SourceType.SMS -> rule.senders.isEmpty() || rule.senders.any { PhoneNumber.same(it, msg.origin) }
            SourceType.NOTIFICATION -> rule.packageName.isNotEmpty() && rule.packageName == msg.origin && titleOk(rule, msg.title)
        }
        return originOk && keywordsOk(rule, msg.fullText)
    }

    /**
     * 켜진 규칙을 순서대로 맞춰 보고 처음 형식이 맞는 규칙으로 보낸다.
     * 형식이 맞는 규칙이 없으면 "원문 보내기"가 켜진 규칙으로 원문만 보내고, 그것도 없으면 Unmatched.
     */
    fun decide(rules: List<Rule>, msg: IncomingMessage, today: LocalDate): Decision {
        val candidates = rules.filter { it.enabled && appliesTo(it, msg) }
        if (candidates.isEmpty()) return Decision.Ignore
        for (rule in candidates) {
            if (rule.parseMode == ParseMode.SERVER) return Decision.Upload(rule, null, payload(rule, msg.fullText, null))
            val format = try {
                compile(rule)
            } catch (_: FormatException) {
                null
            } ?: continue
            val parsed = format.extract(msg.fullText, today) ?: continue
            return Decision.Upload(rule, parsed, payload(rule, msg.fullText, parsed))
        }
        candidates.firstOrNull { it.sendRawIfUnmatched }?.let {
            return Decision.Upload(it, null, payload(it, msg.fullText, null))
        }
        return Decision.Unmatched(candidates.first())
    }

    /** 규칙 화면에서 샘플 하나로 시험 (번호·앱은 보지 않는다) */
    fun test(rule: Rule, sample: String, today: LocalDate): TestResult {
        val title = if (rule.source == SourceType.NOTIFICATION) sample.lineSequence().firstOrNull().orEmpty() else ""
        val keywordMissing = !keywordsOk(rule, sample)
        val titleMissing = rule.source == SourceType.NOTIFICATION && !titleOk(rule, title)
        val format = try {
            compile(rule)
        } catch (e: FormatException) {
            return TestResult(e.message, keywordMissing, titleMissing, null, null)
        }
        if (sample.isBlank()) return TestResult(null, keywordMissing, titleMissing, null, null)
        val parsed = format?.extract(sample, today)
        val payload = when {
            keywordMissing || titleMissing -> null
            format == null -> payload(rule, sample, null)
            parsed != null -> payload(rule, sample, parsed)
            rule.sendRawIfUnmatched -> payload(rule, sample, null)
            else -> null
        }
        val card = if (payload != null) cardChoice(rule, parsed) else null
        return TestResult(null, keywordMissing, titleMissing, parsed, payload, card)
    }

    /**
     * 서버 `/api/ingest` 요청 본문. 원문(`text`)은 항상 보내고, 형식에서 뽑은 값은 명시 필드로 넣는다
     * (서버는 명시 필드를 먼저 쓰고 빠진 값은 원문에서 찾는다. 중복 방지는 원문 해시).
     */
    fun payload(rule: Rule?, text: String, parsed: ParsedFields?): JsonObject = buildJsonObject {
        put("text", text)
        parsed?.amount?.let { put("amount", it) }
        parsed?.merchant?.let { put("merchant", it) }
        parsed?.date?.let { put("date", it.toString()) }
        parsed?.installmentMonths?.let { put("installmentMonths", it) }
        cardChoice(rule, parsed)?.let { put("card", it.value) }
        if (rule?.cancel == true) put("cancel", true)
        rule?.ledger?.trim()?.takeIf(String::isNotEmpty)?.let { put("ledger", it) }
        rule?.category?.trim()?.takeIf(String::isNotEmpty)?.let { put("category", it) }
    }
}
