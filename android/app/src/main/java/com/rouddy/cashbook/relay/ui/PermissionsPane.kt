package com.rouddy.cashbook.relay.ui

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import com.rouddy.cashbook.relay.data.AppConfig
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.source.MmsWorker
import kotlinx.coroutines.launch

/** 화면이 다시 보일 때마다(설정 앱에서 돌아올 때) 새로 읽는 권한 상태와, 직접 새로 읽기 */
@Composable
fun rememberPermissionState(): Pair<PermissionState, () -> Unit> {
    val context = LocalContext.current
    var tick by remember { mutableIntStateOf(0) }
    LifecycleResumeEffect(Unit) {
        tick++
        onPauseOrDispose { }
    }
    val state = remember(tick) { PermissionState.of(context) }
    return state to { tick++ }
}

/** 켜 둔 규칙에 필요한 권한 */
data class PermissionNeeds(val sms: Boolean, val notifications: Boolean) {
    fun missing(p: PermissionState): Boolean = (sms && !p.receiveSms) || (notifications && !p.notificationAccess)

    companion object {
        fun of(config: AppConfig?) = PermissionNeeds(
            sms = config?.rules?.any { it.enabled && it.source == SourceType.SMS } == true,
            notifications = config?.rules?.any { it.enabled && it.source == SourceType.NOTIFICATION } == true,
        )
    }
}

/** 설정 → 권한: 자동 입력에 필요한 권한·설정 */
@Composable
fun PermissionsPane(contentPadding: PaddingValues, config: AppConfig?) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val (permissions, refresh) = rememberPermissionState()
    val needs = PermissionNeeds.of(config)
    val smsLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        refresh()
        scope.launch { MmsWorker.initCursor(context) }
    }
    val notifyLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { refresh() }
    val onSmsPermission = { smsLauncher.launch(PermissionState.SMS_PERMISSIONS) }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(contentPadding)) {
        ElevatedCard(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
            Column(Modifier.padding(vertical = 8.dp)) {
                StatusRow(
                    permissions.receiveSms,
                    needs.sms,
                    "문자 받기",
                    if (permissions.receiveSms) "허용됨" else "문자 규칙을 쓰려면 필요",
                    "허용",
                    onSmsPermission,
                )
                StatusRow(
                    permissions.readSms,
                    needs.sms,
                    "문자 읽기",
                    if (permissions.readSms) "허용됨 — 장문(LMS) 승인 문자도 받습니다" else "장문(LMS) 문자와 받은 번호 목록에 필요",
                    "허용",
                    onSmsPermission,
                )
                StatusRow(
                    permissions.notificationAccess,
                    needs.notifications,
                    "알림 접근",
                    if (permissions.notificationAccess) "허용됨" else "앱 알림 규칙을 쓰려면 필요",
                    "설정 열기",
                ) { openNotificationAccessSettings(context) }
                StatusRow(
                    permissions.batteryUnrestricted,
                    false,
                    "배터리 최적화 제외",
                    if (permissions.batteryUnrestricted) "제외됨" else "알림을 놓치지 않으려면 권장",
                    "제외",
                ) { requestBatteryExemption(context) }
                StatusRow(
                    permissions.postNotifications,
                    false,
                    "실패 알림",
                    if (permissions.postNotifications) "업로드에 실패하면 알려 줍니다" else "업로드 실패를 알림으로 받으려면 허용",
                    "허용",
                ) {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) notifyLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            }
        }
        if (needs.missing(permissions)) {
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "허용 버튼이 막혀 있으면 앱 정보 → ⋮ → '제한된 설정 허용' 후 다시 시도하세요.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { openAppDetails(context) }) { Text("앱 정보") }
            }
        }
        Text(
            "빨간 표시는 켜 둔 규칙에 꼭 필요한 권한입니다. 문자 규칙만 쓰면 알림 접근은, 앱 알림 규칙만 쓰면 문자 권한은 필요 없습니다.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(16.dp),
        )
    }
}

@Composable
private fun StatusRow(ok: Boolean, required: Boolean, title: String, detail: String, action: String, onAction: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(
            if (ok) Icons.Default.CheckCircle else Icons.Default.Warning,
            contentDescription = if (ok) "정상" else "필요",
            tint = when {
                ok -> MaterialTheme.colorScheme.primary
                required -> MaterialTheme.colorScheme.error
                else -> MaterialTheme.colorScheme.outline
            },
            modifier = Modifier.size(20.dp),
        )
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyMedium)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        if (!ok) TextButton(onClick = onAction) { Text(action) }
    }
}
