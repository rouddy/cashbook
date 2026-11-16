package com.rouddy.cashbook.relay.ui

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.ParseMode
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.data.UploadStatus
import com.rouddy.cashbook.relay.ingest.RuleMatcher
import com.rouddy.cashbook.relay.parse.PhoneNumber
import com.rouddy.cashbook.relay.source.MmsWorker
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(
    onEdit: (Rule) -> Unit,
    onAdd: (SourceType) -> Unit,
    onHistory: () -> Unit,
    onSettings: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val config by Graph.store.configFlow.collectAsStateWithLifecycle(initialValue = null)
    val records by Graph.store.recordsFlow.collectAsStateWithLifecycle(initialValue = emptyList())
    val failed = records.count { it.status == UploadStatus.FAILED }

    var refresh by remember { mutableIntStateOf(0) }
    LifecycleResumeEffect(Unit) {
        refresh++
        onPauseOrDispose { }
    }
    val permissions = remember(refresh) { PermissionState.of(context) }
    val smsLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        refresh++
        scope.launch { MmsWorker.initCursor(context) }
    }
    val notifyLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { refresh++ }

    var addMenu by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf<Rule?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("가계부 자동 입력") },
                actions = {
                    IconButton(onClick = onHistory) {
                        BadgedBox(badge = { if (failed > 0) Badge { Text("$failed") } }) {
                            Icon(Icons.AutoMirrored.Filled.List, contentDescription = "업로드 기록")
                        }
                    }
                    IconButton(onClick = onSettings) { Icon(Icons.Default.Settings, contentDescription = "서버 설정") }
                },
            )
        },
        floatingActionButton = {
            Box {
                ExtendedFloatingActionButton(
                    onClick = { addMenu = true },
                    icon = { Icon(Icons.Default.Add, contentDescription = null) },
                    text = { Text("규칙 추가") },
                )
                DropdownMenu(expanded = addMenu, onDismissRequest = { addMenu = false }) {
                    DropdownMenuItem(
                        text = { Text("문자 규칙") },
                        leadingIcon = { Icon(Icons.Default.Email, contentDescription = null) },
                        onClick = {
                            addMenu = false
                            onAdd(SourceType.SMS)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("앱 알림 규칙") },
                        leadingIcon = { Icon(Icons.Default.Notifications, contentDescription = null) },
                        onClick = {
                            addMenu = false
                            onAdd(SourceType.NOTIFICATION)
                        },
                    )
                }
            }
        },
    ) { padding ->
        val current = config ?: return@Scaffold
        val smsRules = current.rules.filter { it.source == SourceType.SMS }
        val notificationRules = current.rules.filter { it.source == SourceType.NOTIFICATION }
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = padding.calculateBottomPadding() + 88.dp),
        ) {
            item {
                StatusCard(
                    serverConfigured = current.settings.serverUrl.isNotBlank(),
                    serverUrl = current.settings.serverUrl,
                    enabled = current.settings.enabled,
                    onEnabled = { on -> scope.launch { Graph.store.updateSettings { it.copy(enabled = on) } } },
                    permissions = permissions,
                    needsSms = smsRules.any { it.enabled },
                    needsNotifications = notificationRules.any { it.enabled },
                    onSettings = onSettings,
                    onSmsPermission = { smsLauncher.launch(PermissionState.SMS_PERMISSIONS) },
                    onNotificationAccess = { openNotificationAccessSettings(context) },
                    onPostNotifications = {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) notifyLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                    },
                    onBattery = { requestBatteryExemption(context) },
                    onAppDetails = { openAppDetails(context) },
                )
            }
            if (current.rules.isEmpty()) {
                item {
                    Text(
                        "규칙이 없습니다. 아래 \"규칙 추가\"로 카드사 문자 번호나 결제 앱 알림을 등록하세요.\n" +
                            "규칙에 맞는 문자/알림만 서버로 보냅니다.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(24.dp),
                    )
                }
            }
            ruleSection("문자", smsRules, onEdit, onDelete = { deleting = it }, scope = scope)
            ruleSection("앱 알림", notificationRules, onEdit, onDelete = { deleting = it }, scope = scope)
        }
    }

    deleting?.let { rule ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text("규칙 삭제") },
            text = { Text("'${rule.displayName()}' 규칙을 지울까요?") },
            confirmButton = {
                TextButton(onClick = {
                    scope.launch { Graph.store.deleteRule(rule.id) }
                    deleting = null
                }) { Text("삭제") }
            },
            dismissButton = { TextButton(onClick = { deleting = null }) { Text("취소") } },
        )
    }
}

fun Rule.displayName(): String = name.ifBlank {
    when (source) {
        SourceType.SMS -> senders.firstOrNull()?.let(PhoneNumber::format) ?: "모든 문자"
        SourceType.NOTIFICATION -> appLabel.ifBlank { packageName.ifBlank { "앱 미선택" } }
    }
}

private fun Rule.summary(): String {
    val target = when (source) {
        SourceType.SMS -> when (senders.size) {
            0 -> "모든 번호"
            1 -> PhoneNumber.format(senders[0])
            else -> "${PhoneNumber.format(senders[0])} 외 ${senders.size - 1}"
        }
        SourceType.NOTIFICATION -> buildString {
            append(appLabel.ifBlank { packageName })
            if (titleContains.isNotBlank()) append(" · 제목 '${titleContains.trim()}'")
        }
    }
    val mode = when (parseMode) {
        ParseMode.TEMPLATE -> "템플릿"
        ParseMode.REGEX -> "정규식"
        ParseMode.SERVER -> "서버 해석"
    }
    val extras = listOfNotNull(
        if (cancel) "취소 문자" else null,
        card.trim().takeIf(String::isNotEmpty)?.let { "카드 $it" },
    )
    // 제목(이름)과 같은 대상은 다시 쓰지 않는다
    val showTarget = name.isNotBlank() && name.trim() != target
    return (listOfNotNull(target.takeIf { showTarget }, mode) + extras).joinToString(" · ")
}

private fun androidx.compose.foundation.lazy.LazyListScope.ruleSection(
    title: String,
    rules: List<Rule>,
    onEdit: (Rule) -> Unit,
    onDelete: (Rule) -> Unit,
    scope: kotlinx.coroutines.CoroutineScope,
) {
    if (rules.isEmpty()) return
    item(key = "header-$title") {
        SectionTitle("$title 규칙 (위에서부터 먼저 맞춰 봄)", Modifier.padding(start = 16.dp, top = 20.dp, bottom = 4.dp))
    }
    items(rules, key = { it.id }) { rule ->
        val index = rules.indexOf(rule)
        RuleItem(
            rule = rule,
            canMoveUp = index > 0,
            canMoveDown = index < rules.lastIndex,
            onClick = { onEdit(rule) },
            onToggle = { on -> scope.launch { Graph.store.setRuleEnabled(rule.id, on) } },
            onMove = { delta -> scope.launch { Graph.store.moveRule(rule.id, delta) } },
            onDelete = { onDelete(rule) },
        )
        HorizontalDivider()
    }
}

@Composable
private fun RuleItem(
    rule: Rule,
    canMoveUp: Boolean,
    canMoveDown: Boolean,
    onClick: () -> Unit,
    onToggle: (Boolean) -> Unit,
    onMove: (Int) -> Unit,
    onDelete: () -> Unit,
) {
    val error = remember(rule) { RuleMatcher.formatError(rule) }
    var menu by remember { mutableStateOf(false) }
    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        leadingContent = { SourceIcon(rule.source, rule.packageName) },
        headlineContent = { Text(rule.displayName(), maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Column {
                Text(rule.summary(), maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (error != null) {
                    Text("형식 오류: $error", color = MaterialTheme.colorScheme.error, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Switch(checked = rule.enabled, onCheckedChange = onToggle)
                Box {
                    IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "더 보기") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(
                            text = { Text("위로") },
                            leadingIcon = { Icon(Icons.Default.KeyboardArrowUp, null) },
                            enabled = canMoveUp,
                            onClick = {
                                menu = false
                                onMove(-1)
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("아래로") },
                            leadingIcon = { Icon(Icons.Default.KeyboardArrowDown, null) },
                            enabled = canMoveDown,
                            onClick = {
                                menu = false
                                onMove(1)
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("삭제") },
                            leadingIcon = { Icon(Icons.Default.Delete, null) },
                            onClick = {
                                menu = false
                                onDelete()
                            },
                        )
                    }
                }
            }
        },
    )
}

@Composable
private fun StatusCard(
    serverConfigured: Boolean,
    serverUrl: String,
    enabled: Boolean,
    onEnabled: (Boolean) -> Unit,
    permissions: PermissionState,
    needsSms: Boolean,
    needsNotifications: Boolean,
    onSettings: () -> Unit,
    onSmsPermission: () -> Unit,
    onNotificationAccess: () -> Unit,
    onPostNotifications: () -> Unit,
    onBattery: () -> Unit,
    onAppDetails: () -> Unit,
) {
    ElevatedCard(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Column(Modifier.padding(vertical = 8.dp)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("자동 업로드", style = MaterialTheme.typography.titleMedium)
                    Text(
                        if (enabled) "규칙에 맞는 문자·알림을 받으면 바로 보냅니다" else "꺼져 있음 — 받은 문자·알림을 처리하지 않습니다",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(checked = enabled, onCheckedChange = onEnabled)
            }
            Spacer(Modifier.size(4.dp))
            StatusRow(serverConfigured, true, "서버", if (serverConfigured) serverUrl else "주소와 API 토큰을 설정하세요", "설정", onSettings)
            StatusRow(
                permissions.receiveSms,
                needsSms,
                "문자 받기",
                if (permissions.receiveSms) "허용됨" else "문자 규칙을 쓰려면 필요",
                "허용",
                onSmsPermission,
            )
            StatusRow(
                permissions.readSms,
                needsSms,
                "문자 읽기",
                if (permissions.readSms) "허용됨 — 장문(LMS) 승인 문자도 받습니다" else "장문(LMS) 문자와 받은 번호 목록에 필요",
                "허용",
                onSmsPermission,
            )
            StatusRow(
                permissions.notificationAccess,
                needsNotifications,
                "알림 접근",
                if (permissions.notificationAccess) "허용됨" else "앱 알림 규칙을 쓰려면 필요",
                "설정 열기",
                onNotificationAccess,
            )
            StatusRow(
                permissions.batteryUnrestricted,
                false,
                "배터리 최적화 제외",
                if (permissions.batteryUnrestricted) "제외됨" else "알림을 놓치지 않으려면 권장",
                "제외",
                onBattery,
            )
            StatusRow(
                permissions.postNotifications,
                false,
                "실패 알림",
                if (permissions.postNotifications) "업로드에 실패하면 알려 줍니다" else "업로드 실패를 알림으로 받으려면 허용",
                "허용",
                onPostNotifications,
            )
            if ((needsSms && !permissions.receiveSms) || (needsNotifications && !permissions.notificationAccess)) {
                Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "허용 버튼이 막혀 있으면 앱 정보 → ⋮ → '제한된 설정 허용' 후 다시 시도하세요.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = onAppDetails) { Text("앱 정보") }
                }
            }
        }
    }
}

@Composable
private fun StatusRow(ok: Boolean, required: Boolean, title: String, detail: String, action: String, onAction: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Start,
    ) {
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
