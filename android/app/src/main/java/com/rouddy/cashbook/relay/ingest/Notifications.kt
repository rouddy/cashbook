package com.rouddy.cashbook.relay.ingest

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.rouddy.cashbook.relay.MainActivity
import com.rouddy.cashbook.relay.R
import com.rouddy.cashbook.relay.data.UploadRecord

object Notifications {
    private const val CHANNEL_FAILED = "upload_failed"
    private const val CHANNEL_WORK = "uploading"
    const val UPLOADING_ID = 1
    const val EXTRA_OPEN_HISTORY = "openHistory"

    fun createChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(CHANNEL_FAILED, "업로드 실패", NotificationManager.IMPORTANCE_DEFAULT).apply {
                    description = "자동 입력을 서버로 보내지 못했을 때"
                },
                NotificationChannel(CHANNEL_WORK, "업로드 중", NotificationManager.IMPORTANCE_MIN).apply {
                    description = "Android 11 이하에서 업로드하는 동안 잠깐 표시"
                },
            ),
        )
    }

    private fun openHistory(context: Context): PendingIntent = PendingIntent.getActivity(
        context,
        0,
        Intent(context, MainActivity::class.java)
            .putExtra(EXTRA_OPEN_HISTORY, true)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    fun uploading(context: Context): Notification =
        NotificationCompat.Builder(context, CHANNEL_WORK)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle("가계부로 보내는 중")
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setOngoing(true)
            .build()

    fun uploadFailed(context: Context, record: UploadRecord) {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED &&
            android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU
        ) {
            return
        }
        val notification = NotificationCompat.Builder(context, CHANNEL_FAILED)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle("자동 입력 실패: ${record.ruleName.ifBlank { record.originLabel }}")
            .setContentText(record.message)
            .setStyle(NotificationCompat.BigTextStyle().bigText("${record.message}\n\n${record.text.take(200)}"))
            .setContentIntent(openHistory(context))
            .setAutoCancel(true)
            .build()
        try {
            NotificationManagerCompat.from(context).notify(record.id.hashCode(), notification)
        } catch (_: SecurityException) {
            // 알림 권한이 없으면 기록 화면에서만 보인다
        }
    }
}
