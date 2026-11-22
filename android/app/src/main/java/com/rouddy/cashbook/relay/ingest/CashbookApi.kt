package com.rouddy.cashbook.relay.ingest

import com.rouddy.cashbook.relay.data.AppJson
import com.rouddy.cashbook.relay.data.ServerOptions
import com.rouddy.cashbook.relay.data.Settings
import com.rouddy.cashbook.relay.data.UploadResult
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.longOrNull
import java.io.IOException
import java.net.HttpURLConnection
import java.net.MalformedURLException
import java.net.URI
import java.net.UnknownServiceException
import javax.net.ssl.SSLException

/**
 * 가계부 서버 API. 자동 입력(`POST /api/ingest`)과 GraphQL(`POST /api/graphql`) — 같은 API 토큰을 쓴다.
 * README의 "자동 입력 API" 참고.
 */
class CashbookApi(serverUrl: String, private val token: String) {
    constructor(settings: Settings) : this(settings.serverUrl, settings.token)

    private val base = serverUrl.trim().trimEnd('/')

    sealed interface Outcome {
        data class Ok(val body: JsonObject) : Outcome

        /** 주소·토큰·내용을 고쳐야 하는 오류 (다시 시도해도 같음) */
        data class Rejected(val status: Int, val message: String) : Outcome

        /** 네트워크·서버 오류. 잠시 뒤 다시 시도 */
        data class Retry(val message: String) : Outcome
    }

    suspend fun ingest(payload: String): Outcome = request("POST", "/api/ingest", payload)

    /** GraphQL 요청. 응답 해석은 [com.rouddy.cashbook.relay.book.BookApi] */
    suspend fun graphql(query: String, variables: JsonObject = JsonObject(emptyMap())): Outcome =
        request("POST", "/api/graphql", buildJsonObject {
            put("query", query)
            put("variables", variables)
        }.toString())

    /** 규칙에서 고를 장부·카테고리·결제수단. 실패하면 보여 줄 메시지 */
    suspend fun options(): Result<ServerOptions> = when (val out = request("GET", "/api/ingest/options", null)) {
        is Outcome.Ok -> parseOptions(out.body)?.let { Result.success(it.copy(serverUrl = base, fetchedAt = System.currentTimeMillis())) }
            ?: Result.failure(IOException("목록 응답을 해석하지 못했습니다"))
        is Outcome.Rejected -> Result.failure(
            IOException(if (out.status == 404) "서버가 목록 API를 지원하지 않습니다. 가계부 서버를 업데이트하세요" else out.message),
        )
        is Outcome.Retry -> Result.failure(IOException(out.message))
    }

    /**
     * 연결과 토큰 확인. 빈 JSON을 자동 입력으로 보내면 인증을 통과한 뒤 "금액 없음"(422)으로 거절되므로
     * 거래를 만들지 않고 토큰을 확인할 수 있다. 이어서 GraphQL 읽기도 되는지 확인한다 (토큰 권한을 합치기 전 서버는 예전 자동 입력 전용 토큰을 막는다).
     */
    suspend fun check(): Pair<Boolean, String> {
        if (base.isEmpty()) return false to "서버 주소를 입력하세요"
        val version = when (val health = request("GET", "/api/health", null)) {
            is Outcome.Ok -> health.body["version"]?.str()
            is Outcome.Rejected -> return false to health.message
            is Outcome.Retry -> return false to health.message
        }
        when (val probe = request("POST", "/api/ingest", "{}")) {
            is Outcome.Rejected -> if (probe.status != 422) return false to probe.message
            is Outcome.Ok -> return false to "서버 응답이 예상과 다릅니다"
            is Outcome.Retry -> return false to probe.message
        }
        val server = version?.let { " (서버 $it)" } ?: ""
        return when (val full = graphql(com.rouddy.cashbook.relay.book.Queries.PROBE)) {
            is Outcome.Ok -> true to "연결됨 · 인증 확인됨$server — 자동 입력과 대시보드·수동 입력·최근 내역을 쓸 수 있습니다"
            is Outcome.Rejected -> if (full.status == 403) {
                true to "연결됨 · 자동 입력 인증 확인됨$server — 이 서버는 이 토큰으로 대시보드·수동 입력·최근 내역을 허용하지 않습니다. 가계부 서버를 업데이트하세요"
            } else {
                true to "연결됨 · 자동 입력 인증 확인됨$server (GraphQL 확인 실패: ${full.message})"
            }
            is Outcome.Retry -> true to "연결됨 · 자동 입력 인증 확인됨$server (GraphQL 확인 실패: ${full.message})"
        }
    }

    private suspend fun request(method: String, path: String, body: String?): Outcome = withContext(Dispatchers.IO) {
        if (base.isEmpty()) return@withContext Outcome.Rejected(0, "서버 주소가 설정되지 않았습니다")
        val conn = try {
            URI(base + path).toURL().openConnection() as HttpURLConnection
        } catch (_: MalformedURLException) {
            return@withContext Outcome.Rejected(0, "서버 주소가 올바르지 않습니다: $base")
        } catch (_: IllegalArgumentException) {
            return@withContext Outcome.Rejected(0, "서버 주소가 올바르지 않습니다: $base")
        } catch (_: java.net.URISyntaxException) {
            return@withContext Outcome.Rejected(0, "서버 주소가 올바르지 않습니다: $base")
        }
        try {
            conn.requestMethod = method
            conn.connectTimeout = 15_000
            conn.readTimeout = 30_000
            conn.instanceFollowRedirects = false
            conn.setRequestProperty("Accept", "application/json")
            if (token.isNotBlank()) conn.setRequestProperty("Authorization", "Bearer ${token.trim()}")
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json; charset=utf-8")
                conn.outputStream.use { it.write(body.encodeToByteArray()) }
            }
            val code = conn.responseCode
            val text = (if (code < 400) conn.inputStream else conn.errorStream)?.use { it.readBytes().decodeToString() }.orEmpty()
            classify(code, text)
        } catch (e: UnknownServiceException) {
            Outcome.Rejected(0, "http 주소는 localhost(adb reverse)와 에뮬레이터(10.0.2.2)에서만 쓸 수 있습니다. https 주소를 쓰세요 (${e.message})")
        } catch (e: SSLException) {
            Outcome.Rejected(0, "HTTPS 연결 오류: ${e.message}")
        } catch (e: IOException) {
            Outcome.Retry("연결 실패: ${e.message ?: e.javaClass.simpleName}")
        } finally {
            conn.disconnect()
        }
    }

    companion object {
        internal fun classify(code: Int, text: String): Outcome {
            val json = runCatching { AppJson.parseToJsonElement(text).jsonObject }.getOrNull()
            val serverError = json?.get("error")?.str()
            return when {
                code in 200..299 && json != null -> Outcome.Ok(json)
                code in 200..299 -> Outcome.Rejected(code, "가계부 서버 응답이 아닙니다. 주소를 확인하세요")
                code in 300..399 -> Outcome.Rejected(code, "주소가 다른 곳으로 넘겨집니다 ($code). https:// 주소를 정확히 입력하세요")
                code == 401 -> Outcome.Rejected(code, "API 토큰이 올바르지 않습니다 (401)")
                code == 403 -> Outcome.Rejected(code, serverError ?: "접근이 거부되었습니다 (403)")
                code == 404 -> Outcome.Rejected(code, "주소에서 가계부 API를 찾지 못했습니다 (404)")
                code == 408 || code == 429 || code >= 500 -> Outcome.Retry(serverError ?: "서버 오류 ($code)")
                else -> Outcome.Rejected(code, serverError ?: "요청이 거부되었습니다 ($code)")
            }
        }

        internal fun parseOptions(body: JsonObject): ServerOptions? =
            runCatching { AppJson.decodeFromJsonElement(ServerOptions.serializer(), body) }.getOrNull()

        /** 자동 입력 응답 → (중복 여부, 보여 줄 값) */
        internal fun parseResult(body: JsonObject): Pair<Boolean, UploadResult> {
            val tx = body["transaction"] as? JsonObject
            val duplicate = (body["duplicate"] as? JsonPrimitive)?.booleanOrNull == true
            return duplicate to UploadResult(
                transactionId = (tx?.get("id") as? JsonPrimitive)?.longOrNull,
                amount = (tx?.get("amount") as? JsonPrimitive)?.longOrNull,
                description = tx?.get("description")?.str(),
                date = tx?.get("date")?.str(),
                category = tx?.get("category")?.str(),
                paymentMethod = tx?.get("paymentMethod")?.str(),
                ledger = tx?.get("ledger")?.str(),
                matchedPlanned = (body["matchedPlanned"] as? JsonPrimitive)?.booleanOrNull == true,
                warnings = runCatching { body["warnings"]?.jsonArray?.mapNotNull { it.str() } }.getOrNull().orEmpty(),
            )
        }

        private fun kotlinx.serialization.json.JsonElement.str(): String? = (this as? JsonPrimitive)?.contentOrNull
    }
}
