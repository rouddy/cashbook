package com.rouddy.cashbook.relay.book

import com.rouddy.cashbook.relay.data.AppJson
import com.rouddy.cashbook.relay.data.Store
import com.rouddy.cashbook.relay.ingest.CashbookApi
import com.rouddy.cashbook.relay.ingest.CashbookApi.Outcome
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put
import java.time.LocalDate
import java.time.YearMonth

/** 가계부 GraphQL 오류. [kind]로 화면이 "서버 설정 열기" 같은 다음 행동을 고른다. */
class BookException(message: String, val kind: Kind) : Exception(message) {
    enum class Kind {
        /** 서버 주소가 없음 */
        NOT_CONFIGURED,

        /** 예전 서버가 이 토큰(예전 '자동 입력 전용')으로 GraphQL을 막음 — 서버 업데이트 필요 */
        TOKEN_NOT_ALLOWED,

        /** 토큰이 틀림·주소 오류 등 설정을 고쳐야 하는 오류 */
        SETTINGS,

        /** 네트워크·서버 오류 (다시 시도) */
        NETWORK,

        /** 입력 오류 등 서버가 돌려준 GraphQL 오류 */
        SERVER,
    }

    /** 서버 설정 화면에서 고칠 수 있는 오류인지 */
    val fixInSettings: Boolean get() = kind == Kind.NOT_CONFIGURED || kind == Kind.TOKEN_NOT_ALLOWED || kind == Kind.SETTINGS
}

object BookApi {
    const val TOKEN_NOT_ALLOWED_MESSAGE = "가계부 서버가 이 토큰으로 대시보드·수동 입력·최근 내역(GraphQL)을 허용하지 않습니다. " +
        "토큰 권한이 하나로 합쳐진 최신 가계부 서버로 업데이트하면 같은 토큰으로 쓸 수 있습니다 (자동 입력은 지금도 됩니다)."

    /** HTTP 결과 → GraphQL `data`를 [deserializer]로 읽은 값. 실패하면 [BookException]. */
    fun <T> interpret(outcome: Outcome, deserializer: DeserializationStrategy<T>): T = when (outcome) {
        is Outcome.Ok -> parse(outcome.body, deserializer)
        is Outcome.Rejected -> throw when {
            // 토큰 권한을 합치기 전 서버의 FullAccess 가드: "자동 입력 전용 토큰은 /api/ingest 에만 쓸 수 있습니다"
            outcome.status == 403 && outcome.message.contains("자동 입력 전용") ->
                BookException(TOKEN_NOT_ALLOWED_MESSAGE, BookException.Kind.TOKEN_NOT_ALLOWED)
            else -> BookException(outcome.message, BookException.Kind.SETTINGS)
        }
        is Outcome.Retry -> throw BookException(outcome.message, BookException.Kind.NETWORK)
    }

    /** GraphQL 응답 본문 (`{"data": …, "errors": […]}`) 해석 */
    fun <T> parse(body: JsonObject, deserializer: DeserializationStrategy<T>): T {
        val errors = body["errors"] as? JsonArray
        if (!errors.isNullOrEmpty()) {
            val message = errors.mapNotNull { ((it as? JsonObject)?.get("message") as? JsonPrimitive)?.contentOrNull }
                .distinct()
                .joinToString("\n")
                .ifBlank { "가계부 서버 오류" }
            throw BookException(message, BookException.Kind.SERVER)
        }
        val data = body["data"] as? JsonObject ?: throw BookException("가계부 서버 응답에 data가 없습니다", BookException.Kind.SERVER)
        return try {
            AppJson.decodeFromJsonElement(deserializer, data)
        } catch (e: SerializationException) {
            throw BookException("응답을 해석하지 못했습니다. 가계부 서버 버전을 확인하세요 (${e.message})", BookException.Kind.SERVER)
        } catch (e: IllegalArgumentException) {
            throw BookException("응답을 해석하지 못했습니다. 가계부 서버 버전을 확인하세요 (${e.message})", BookException.Kind.SERVER)
        }
    }

    /** 대시보드의 예정 거래 범위: 석 달 전부터(지난 미확정) 앞으로 [aheadDays]일 */
    fun plannedRange(today: LocalDate, aheadDays: Long = 7): Pair<LocalDate, LocalDate> = today.minusDays(92) to today.plusDays(aheadDays)
}

/**
 * 최근 내역 필터. 확정 거래만 (예정은 대시보드에서).
 * [autoOnly]: 자동 입력(source = API)만, [uncategorizedOnly]: 항목 없는 거래만 (서버 `uncategorized` — 장부 이체 제외)
 */
data class RecentFilter(val autoOnly: Boolean = false, val uncategorizedOnly: Boolean = false) {
    /** `TransactionFilter` */
    fun toJson(): JsonObject = buildJsonObject {
        put("statuses", JsonArray(listOf(JsonPrimitive("CONFIRMED"))))
        if (autoOnly) put("source", "API")
        if (uncategorizedOnly) put("uncategorized", true)
    }

    /** 서버 필터와 같은 기준: 고친 거래가 아직 이 목록에 남는지 (항목을 정하면 '미분류만'에서 빠진다) */
    fun matches(tx: Tx): Boolean =
        tx.status == "CONFIRMED" &&
            (!autoOnly || tx.isAuto) &&
            (!uncategorizedOnly || (tx.categoryId == null && !tx.isTransfer))
}

@Serializable
private data class TransactionsData(val transactions: TxPage)

@Serializable
private data class SuggestionsData(val descriptionSuggestions: List<Suggestion>)

@Serializable
private data class CreateData(val createTransaction: SavedTx)

@Serializable
private data class UpdateData(val updateTransaction: Tx)

@Serializable
private data class ConfirmData(val confirmTransaction: SavedTx)

@Serializable
private data class SkipData(val skipTransaction: SavedTx)

/**
 * 가계부 데이터(대시보드·수동 입력·최근 내역). 요청마다 저장된 서버 설정을 읽는다.
 * 작은 목록([ref])은 기억해 두고, 거래를 바꾸면 [changes]를 올려 화면들이 다시 읽게 한다.
 */
class BookRepository(private val store: Store) {
    private val _ref = MutableStateFlow<RefData?>(null)

    /** 마지막으로 받은 장부·카테고리·결제수단 */
    val ref: StateFlow<RefData?> = _ref.asStateFlow()

    private val _changes = MutableStateFlow(0)

    /** 거래가 바뀌면(수동 입력, 예정 확정, 자동 입력 성공, 서버 설정 변경) 1씩 오른다 */
    val changes: StateFlow<Int> = _changes.asStateFlow()

    fun notifyChanged() = _changes.update { it + 1 }

    /** 서버 설정이 바뀌면 다른 서버의 목록을 버린다 */
    fun reset() {
        _ref.value = null
        notifyChanged()
    }

    private suspend fun <T> query(query: String, variables: JsonObject, deserializer: KSerializer<T>): T {
        val settings = store.config().settings
        if (settings.serverUrl.isBlank()) {
            throw BookException("서버 설정에서 가계부 주소와 API 토큰을 넣으세요", BookException.Kind.NOT_CONFIGURED)
        }
        return BookApi.interpret(CashbookApi(settings).graphql(query, variables), deserializer)
    }

    suspend fun loadRef(): RefData = query(Queries.REF, JsonObject(emptyMap()), RefData.serializer()).also { _ref.value = it }

    suspend fun refOrLoad(): RefData = _ref.value ?: loadRef()

    suspend fun dashboard(month: YearMonth, ledgerId: Int?, today: LocalDate): Dashboard {
        val (from, to) = BookApi.plannedRange(today)
        val variables = buildJsonObject {
            put("month", month.toString())
            put("ledgerId", ledgerId?.let(::JsonPrimitive) ?: JsonNull)
            put("from", from.toString())
            put("to", to.toString())
        }
        return query(Queries.DASHBOARD, variables, Dashboard.serializer())
    }

    suspend fun transactions(filter: RecentFilter, offset: Int, limit: Int): TxPage {
        val variables = buildJsonObject {
            put("filter", filter.toJson())
            put("offset", offset)
            put("limit", limit)
        }
        return query(Queries.TRANSACTIONS, variables, TransactionsData.serializer()).transactions
    }

    /** 최근 1년 안에 쓴 비슷한 내용 */
    suspend fun suggestions(text: String, today: LocalDate): List<Suggestion> {
        val variables = buildJsonObject {
            put("query", text)
            put("since", today.minusYears(1).toString())
        }
        return query(Queries.SUGGESTIONS, variables, SuggestionsData.serializer()).descriptionSuggestions
    }

    suspend fun create(input: JsonObject): SavedTx =
        query(Queries.CREATE_TX, buildJsonObject { put("input", input) }, CreateData.serializer()).createTransaction
            .also { notifyChanged() }

    /** [patch]는 [TxEditRules.toPatch]가 만든 바뀐 값만 */
    suspend fun update(id: Int, patch: JsonObject): Tx {
        val variables = buildJsonObject {
            put("id", id)
            put("input", patch)
        }
        return query(Queries.UPDATE_TX, variables, UpdateData.serializer()).updateTransaction.also { notifyChanged() }
    }

    suspend fun confirm(id: Int): SavedTx =
        query(Queries.CONFIRM_TX, buildJsonObject { put("id", id) }, ConfirmData.serializer()).confirmTransaction
            .also { notifyChanged() }

    suspend fun skip(id: Int): SavedTx =
        query(Queries.SKIP_TX, buildJsonObject { put("id", id) }, SkipData.serializer()).skipTransaction
            .also { notifyChanged() }
}
