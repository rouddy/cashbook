package com.rouddy.cashbook.relay.ingest

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.UploadStatus
import java.util.concurrent.TimeUnit

/** 기록 하나를 서버로 보낸다. 네트워크·서버 오류는 지수 백오프로 [MAX_ATTEMPTS]번까지 다시 시도한다. */
class UploadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {

    override suspend fun doWork(): Result {
        val id = inputData.getString(KEY_ID) ?: return Result.failure()
        val store = Graph.store
        val record = store.record(id) ?: return Result.success()
        val payload = record.payload
        if (record.status != UploadStatus.PENDING || payload == null) return Result.success()

        val settings = store.config().settings
        val attempt = runAttemptCount + 1
        when (val outcome = CashbookApi(settings).ingest(payload)) {
            is CashbookApi.Outcome.Ok -> {
                val (duplicate, result) = CashbookApi.parseResult(outcome.body)
                store.updateRecord(id) {
                    it.copy(
                        status = if (duplicate) UploadStatus.DUPLICATE else UploadStatus.SUCCESS,
                        attempts = attempt,
                        message = null,
                        result = result,
                    )
                }
                // 열려 있는 대시보드·최근 내역이 새 거래를 다시 읽는다
                if (!duplicate) Graph.book.notifyChanged()
                return Result.success()
            }
            is CashbookApi.Outcome.Rejected -> {
                fail(id, attempt, outcome.message)
                return Result.success()
            }
            is CashbookApi.Outcome.Retry -> {
                if (attempt >= MAX_ATTEMPTS) {
                    fail(id, attempt, "${outcome.message} — ${attempt}번 시도 후 중단")
                    return Result.success()
                }
                store.updateRecord(id) { it.copy(attempts = attempt, message = "${outcome.message} (다시 시도 예정)") }
                return Result.retry()
            }
        }
    }

    private suspend fun fail(id: String, attempt: Int, message: String) {
        val failed = Graph.store.updateRecord(id) { it.copy(status = UploadStatus.FAILED, attempts = attempt, message = message) }
        if (failed != null) Notifications.uploadFailed(applicationContext, failed)
    }

    /** Android 11 이하에서 긴급(expedited) 작업을 포그라운드로 돌릴 때만 쓴다. (12부터는 JobScheduler가 처리) */
    override suspend fun getForegroundInfo(): ForegroundInfo =
        ForegroundInfo(Notifications.UPLOADING_ID, Notifications.uploading(applicationContext))

    companion object {
        private const val KEY_ID = "recordId"
        const val MAX_ATTEMPTS = 10

        fun enqueue(context: Context, recordId: String) {
            val request = OneTimeWorkRequestBuilder<UploadWorker>()
                .setInputData(workDataOf(KEY_ID to recordId))
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
                .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork("upload-$recordId", ExistingWorkPolicy.REPLACE, request)
        }
    }
}
