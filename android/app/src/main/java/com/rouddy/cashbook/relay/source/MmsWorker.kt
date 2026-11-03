package com.rouddy.cashbook.relay.source

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.ingest.IncomingMessage
import com.rouddy.cashbook.relay.ingest.Ingestor
import java.util.concurrent.TimeUnit

/**
 * 받은 장문(MMS 받은편지함)에서 마지막으로 처리한 _id 이후의 메시지를 읽어 규칙에 넘긴다.
 * 아직 내려받는 중인 메시지가 있으면 그 앞까지만 처리하고 잠시 뒤 다시 본다.
 * 이미 처리한 원문은 기록의 중복 키로 걸러지므로 같은 메시지를 다시 읽어도 두 번 보내지 않는다.
 */
class MmsWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        val context = applicationContext
        if (!MessageReader.canRead(context)) return Result.success()
        val store = Graph.store
        val lastId = store.config().mmsLastId
        if (lastId < 0) {
            // 처음이면 지금까지 받은 메시지는 건너뛰되, 방금(10분 안) 받은 것은 처리한다
            initCursor(context)
        }
        val since = if (lastId < 0) System.currentTimeMillis() - 10 * 60_000L else null
        val entries = MessageReader.mmsInbox(context, afterId = lastId.coerceAtLeast(0), sinceMillis = since)
        var waiting: Long? = null
        for (entry in entries) {
            if (!entry.downloaded) {
                if (waiting == null) waiting = entry.id
                continue
            }
            val message = MessageReader.mmsMessage(context, entry) ?: continue
            if (message.body.isBlank()) continue
            Ingestor.handle(context, IncomingMessage(SourceType.SMS, message.address, body = message.body, receivedAt = message.date))
        }
        val newLast = when {
            waiting != null -> waiting - 1
            entries.isNotEmpty() -> entries.last().id
            else -> null
        }
        if (newLast != null && newLast > store.config().mmsLastId) store.setMmsLastId(newLast)
        return if (waiting != null && runAttemptCount < 6) Result.retry() else Result.success()
    }

    companion object {
        /** 기준이 없으면 지금 받은편지함의 마지막 _id로 잡는다 (설치 전 메시지를 올리지 않도록) */
        suspend fun initCursor(context: Context) {
            val store = Graph.store
            if (store.config().mmsLastId >= 0 || !MessageReader.canRead(context)) return
            store.setMmsLastId(MessageReader.mmsMaxId(context).coerceAtLeast(0))
        }

        fun enqueue(context: Context, delaySeconds: Long = 0) {
            val request = OneTimeWorkRequestBuilder<MmsWorker>()
                .setInitialDelay(delaySeconds, TimeUnit.SECONDS)
                .setBackoffCriteria(BackoffPolicy.LINEAR, 20, TimeUnit.SECONDS)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork("mms-scan", ExistingWorkPolicy.APPEND_OR_REPLACE, request)
        }
    }
}
