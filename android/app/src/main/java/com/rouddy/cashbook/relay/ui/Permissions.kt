package com.rouddy.cashbook.relay.ui

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import com.rouddy.cashbook.relay.source.NotificationCaptureService

/** 자동 입력에 필요한 권한·설정 상태 */
data class PermissionState(
    /** 문자 받기 (RECEIVE_SMS) */
    val receiveSms: Boolean,
    /** 문자 읽기 (READ_SMS + RECEIVE_MMS) — 장문(LMS), 보낸 번호 목록 */
    val readSms: Boolean,
    /** 알림 접근 (NotificationListenerService) */
    val notificationAccess: Boolean,
    /** 업로드 실패 알림 표시 (Android 13+) */
    val postNotifications: Boolean,
    val batteryUnrestricted: Boolean,
) {
    companion object {
        val SMS_PERMISSIONS = arrayOf(Manifest.permission.RECEIVE_SMS, Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_MMS)

        private fun granted(context: Context, permission: String) =
            ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

        fun of(context: Context) = PermissionState(
            receiveSms = granted(context, Manifest.permission.RECEIVE_SMS),
            readSms = granted(context, Manifest.permission.READ_SMS) && granted(context, Manifest.permission.RECEIVE_MMS),
            notificationAccess = NotificationCaptureService.isEnabled(context),
            postNotifications = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || granted(context, Manifest.permission.POST_NOTIFICATIONS),
            batteryUnrestricted = context.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(context.packageName),
        )
    }
}

private fun Context.startFirst(vararg intents: Intent) {
    for (intent in intents) {
        try {
            startActivity(intent)
            return
        } catch (_: ActivityNotFoundException) {
            // 다음 후보
        } catch (_: SecurityException) {
            // 다음 후보
        }
    }
}

fun openNotificationAccessSettings(context: Context) {
    val detail = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        Intent(Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS)
            .putExtra(Settings.EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME, NotificationCaptureService.component(context).flattenToString())
    } else {
        null
    }
    context.startFirst(*listOfNotNull(detail, Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)).toTypedArray())
}

fun openAppDetails(context: Context) {
    context.startFirst(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, "package:${context.packageName}".toUri()))
}

@android.annotation.SuppressLint("BatteryLife")
fun requestBatteryExemption(context: Context) {
    context.startFirst(
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${context.packageName}".toUri()),
        Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
    )
}
