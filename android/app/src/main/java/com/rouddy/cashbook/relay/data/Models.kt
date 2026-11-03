package com.rouddy.cashbook.relay.data

import kotlinx.serialization.Serializable
import java.util.UUID

@Serializable
enum class SourceType { SMS, NOTIFICATION }

@Serializable
enum class ParseMode {
    /** `{금액}`, `{가맹점}` 같은 항목을 넣은 템플릿 */
    TEMPLATE,

    /** 이름 있는 그룹을 쓴 정규식 */
    REGEX,

    /** 원문만 보내고 서버의 카드 승인 문자 해석을 쓴다 */
    SERVER,
}

/**
 * 자동 입력 규칙 하나. 같은 출처(문자/알림)의 규칙은 목록 순서대로 맞춰 보고 처음 맞는 규칙을 쓴다.
 */
@Serializable
data class Rule(
    val id: String = UUID.randomUUID().toString(),
    val name: String = "",
    val enabled: Boolean = true,
    val source: SourceType = SourceType.SMS,
    /** 문자: 보내는 번호(비우면 모든 번호) */
    val senders: List<String> = emptyList(),
    /** 알림: 앱 패키지 이름과 표시 이름 */
    val packageName: String = "",
    val appLabel: String = "",
    /** 알림: 제목(보낸 사람)에 이 글자가 있을 때만 (카카오톡처럼 여러 곳의 알림이 오는 앱) */
    val titleContains: String = "",
    /** 쉼표로 여러 개. 하나라도 본문에 있어야 처리 */
    val keywords: String = "",
    val parseMode: ParseMode = ParseMode.TEMPLATE,
    val format: String = "",
    /** 형식이 맞지 않아도 원문을 보내 서버가 해석하게 한다 */
    val sendRawIfUnmatched: Boolean = false,
    /** 이 형식은 취소/환불 문자 (금액을 음수로 저장) */
    val cancel: Boolean = false,
    /** 서버로 보낼 결제수단(카드 이름·별칭·끝 4자리), 장부, 카테고리. 비우면 서버가 찾는다 */
    val card: String = "",
    val ledger: String = "",
    val category: String = "",
    /** 규칙 화면의 시험용 샘플 (기기에만 저장) */
    val sample: String = "",
)

@Serializable
data class Settings(
    /** 가계부 서버 주소 (예: https://cash.example.com) */
    val serverUrl: String = "",
    /** 가계부 설정 화면에서 발급한 API 토큰 (cb_...) */
    val token: String = "",
    /** 전체 자동 업로드 스위치 */
    val enabled: Boolean = true,
)

@Serializable
data class AppConfig(
    val settings: Settings = Settings(),
    val rules: List<Rule> = emptyList(),
    /** 처리한 마지막 장문(MMS) 메시지 _id. -1이면 아직 기준을 잡지 않음 */
    val mmsLastId: Long = -1,
)

@Serializable
enum class UploadStatus {
    /** 업로드 대기(재시도 포함) */
    PENDING,
    SUCCESS,

    /** 서버에 이미 같은 원문이 있음 */
    DUPLICATE,
    FAILED,

    /** 규칙 대상이지만 형식이 맞지 않아 보내지 않음 */
    SKIPPED,
}

/** 서버 응답에서 보여 줄 값 */
@Serializable
data class UploadResult(
    val transactionId: Long? = null,
    val amount: Long? = null,
    val description: String? = null,
    val date: String? = null,
    val category: String? = null,
    val paymentMethod: String? = null,
    val ledger: String? = null,
    val matchedPlanned: Boolean = false,
    val warnings: List<String> = emptyList(),
)

@Serializable
data class UploadRecord(
    val id: String = UUID.randomUUID().toString(),
    val receivedAt: Long,
    val source: SourceType,
    /** 발신 번호 또는 앱 패키지 */
    val origin: String,
    /** 화면 표시용 (번호 또는 앱 이름) */
    val originLabel: String,
    val ruleId: String?,
    val ruleName: String,
    /** 받은 원문 (알림은 "제목\n본문") */
    val text: String,
    val dedupeKey: String,
    /** 서버로 보낼 JSON. 건너뛴 기록은 null */
    val payload: String?,
    val status: UploadStatus,
    val attempts: Int = 0,
    /** 실패/건너뜀 사유 */
    val message: String? = null,
    val result: UploadResult? = null,
    val updatedAt: Long = receivedAt,
)

@Serializable
data class History(val records: List<UploadRecord> = emptyList())
