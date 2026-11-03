package com.rouddy.cashbook.relay.ingest

import android.content.Context
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.data.UploadRecord
import com.rouddy.cashbook.relay.data.UploadStatus
import com.rouddy.cashbook.relay.parse.PhoneNumber
import java.time.LocalDate

/**
 * 받은 문자/알림 처리: 규칙 맞추기 → 기록 남기기(중복이면 무시) → 업로드 작업 예약.
 * 실제 전송은 [UploadWorker]가 네트워크가 있을 때 하고, 실패하면 다시 시도한다.
 */
object Ingestor {

    suspend fun handle(context: Context, msg: IncomingMessage) {
        val store = Graph.store
        val config = store.config()
        if (!config.settings.enabled || msg.fullText.isBlank()) return
        val decision = RuleMatcher.decide(config.rules, msg, LocalDate.now())
        val rule = when (decision) {
            Decision.Ignore -> return
            is Decision.Unmatched -> decision.rule
            is Decision.Upload -> decision.rule
        }
        val record = UploadRecord(
            receivedAt = msg.receivedAt,
            source = msg.source,
            origin = msg.origin,
            originLabel = when (msg.source) {
                SourceType.SMS -> PhoneNumber.format(msg.origin)
                SourceType.NOTIFICATION -> rule.appLabel.ifBlank { msg.origin }
            },
            ruleId = rule.id,
            ruleName = rule.name,
            text = msg.fullText,
            dedupeKey = msg.dedupeKey,
            payload = (decision as? Decision.Upload)?.payload?.toString(),
            status = if (decision is Decision.Upload) UploadStatus.PENDING else UploadStatus.SKIPPED,
            message = if (decision is Decision.Upload) null else "형식과 맞지 않아 보내지 않았습니다",
        )
        if (store.addRecord(record) && record.status == UploadStatus.PENDING) {
            UploadWorker.enqueue(context, record.id)
        }
    }

    /**
     * 기록을 다시 보낸다. 건너뛴 기록은 원문을 그대로(규칙의 결제수단·장부·카테고리와 함께) 보낸다.
     */
    suspend fun resend(context: Context, id: String) {
        val store = Graph.store
        val rules = store.config().rules
        val updated = store.updateRecord(id) { r ->
            val payload = r.payload ?: RuleMatcher.payload(rules.firstOrNull { it.id == r.ruleId }, r.text, null).toString()
            r.copy(payload = payload, status = UploadStatus.PENDING, attempts = 0, message = null, result = null)
        } ?: return
        UploadWorker.enqueue(context, updated.id)
    }

    suspend fun resendFailed(context: Context): Int {
        val failed = Graph.store.records().filter { it.status == UploadStatus.FAILED }
        failed.forEach { resend(context, it.id) }
        return failed.size
    }
}
