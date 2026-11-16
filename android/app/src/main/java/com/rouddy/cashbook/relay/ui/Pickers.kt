package com.rouddy.cashbook.relay.ui

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.parse.PhoneNumber
import com.rouddy.cashbook.relay.source.MessageReader
import com.rouddy.cashbook.relay.source.RecentNotifications
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.text.Collator
import java.util.Locale

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FullScreenPicker(title: String, onDismiss: () -> Unit, content: @Composable () -> Unit) {
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize()) {
            Scaffold(
                topBar = {
                    TopAppBar(
                        title = { Text(title) },
                        navigationIcon = { IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, contentDescription = "닫기") } },
                    )
                },
            ) { padding ->
                Box(Modifier.padding(padding).fillMaxSize()) { content() }
            }
        }
    }
}

@Composable
private fun Loading() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
}

@Composable
private fun EmptyText(text: String) {
    Text(text, Modifier.padding(24.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
}

private data class InstalledApp(val packageName: String, val label: String)

/** 설치된 앱 목록에서 고른다. 최근 알림이 온 앱을 위에 보여 주고, 고르면 그 앱의 최근 알림을 샘플로 넘긴다. */
@Composable
fun AppPickerDialog(onDismiss: () -> Unit, onPick: (packageName: String, label: String, sample: String?) -> Unit) {
    val context = LocalContext.current
    var query by remember { mutableStateOf("") }
    val recent by RecentNotifications.items.collectAsStateWithLifecycle()
    val apps by produceState<List<InstalledApp>?>(null) {
        value = withContext(Dispatchers.IO) {
            val pm = context.packageManager
            val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
            val collator = Collator.getInstance(Locale.KOREAN)
            pm.queryIntentActivities(launcher, 0)
                .map { it.activityInfo.packageName }
                .distinct()
                .filter { it != context.packageName }
                .map { InstalledApp(it, appLabel(context, it) ?: it) }
                .sortedWith { a, b -> collator.compare(a.label, b.label) }
        }
    }

    FullScreenPicker("앱 선택", onDismiss) {
        val all = apps
        if (all == null) {
            Loading()
            return@FullScreenPicker
        }
        val q = query.trim()
        fun matches(app: InstalledApp) = q.isEmpty() || app.label.contains(q, ignoreCase = true) || app.packageName.contains(q, ignoreCase = true)
        val latestByPackage = recent.groupBy { it.origin }.mapValues { it.value.first() }
        val recentApps = latestByPackage.keys.map { pkg -> all.firstOrNull { it.packageName == pkg } ?: InstalledApp(pkg, appLabel(context, pkg) ?: pkg) }
            .filter(::matches)
        val others = all.filter { it.packageName !in latestByPackage && matches(it) }

        Column {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
                placeholder = { Text("앱 이름 또는 패키지") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            )
            LazyColumn {
                if (recentApps.isNotEmpty()) {
                    item { SectionTitle("최근 알림이 온 앱", Modifier.padding(start = 16.dp, top = 8.dp)) }
                    items(recentApps, key = { "recent-" + it.packageName }) { app ->
                        val latest = latestByPackage[app.packageName]
                        AppRow(app, latest?.fullText) { onPick(app.packageName, app.label, latest?.fullText) }
                    }
                    item { SectionTitle("모든 앱", Modifier.padding(start = 16.dp, top = 16.dp)) }
                }
                items(others, key = { it.packageName }) { app ->
                    AppRow(app, null) { onPick(app.packageName, app.label, null) }
                }
            }
        }
    }
}

@Composable
private fun AppRow(app: InstalledApp, preview: String?, onClick: () -> Unit) {
    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        leadingContent = { AppIcon(app.packageName) },
        headlineContent = { Text(app.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Text(
                preview?.replace('\n', ' ') ?: app.packageName,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        },
    )
}

/** 문자 읽기 권한이 없으면 요청 화면을 보여 준다. 권한이 생기면 [content]를 그린다. */
@Composable
private fun RequireReadSms(content: @Composable (refresh: Int) -> Unit) {
    val context = LocalContext.current
    var refresh by remember { mutableIntStateOf(0) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { refresh++ }
    if (remember(refresh) { MessageReader.canRead(context) }) {
        content(refresh)
    } else {
        Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("받은 문자를 보려면 문자 읽기 권한이 필요합니다. 문자는 기기 안에서만 읽고, 규칙에 맞는 문자만 서버로 보냅니다.")
            Button(onClick = { launcher.launch(PermissionState.SMS_PERMISSIONS) }) { Text("권한 허용") }
        }
    }
}

/** 받은 문자의 보낸 번호 목록에서 고른다. 고른 번호의 최근 문자를 샘플로 넘긴다. */
@Composable
fun SenderPickerDialog(onDismiss: () -> Unit, onPick: (address: String, sample: String) -> Unit) {
    val context = LocalContext.current
    var query by remember { mutableStateOf("") }
    FullScreenPicker("받은 문자에서 번호 고르기", onDismiss) {
        RequireReadSms { refresh ->
            val senders by produceState<List<MessageReader.Sender>?>(null, refresh) {
                value = withContext(Dispatchers.IO) { MessageReader.senders(context) }
            }
            val list = senders
            if (list == null) {
                Loading()
                return@RequireReadSms
            }
            val q = query.trim()
            val shown = list.filter {
                q.isEmpty() || it.address.contains(q) || PhoneNumber.normalize(it.address).contains(PhoneNumber.normalize(q)) ||
                    it.latest.body.contains(q, ignoreCase = true)
            }
            Column {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    leadingIcon = { Icon(Icons.Default.Search, contentDescription = null) },
                    placeholder = { Text("번호 또는 내용 (예: 승인)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                )
                if (shown.isEmpty()) EmptyText("받은 문자가 없습니다.")
                LazyColumn {
                    items(shown, key = { it.address }) { sender ->
                        ListItem(
                            modifier = Modifier.clickable { onPick(sender.address, sender.latest.body) },
                            headlineContent = { Text(PhoneNumber.format(sender.address)) },
                            supportingContent = { Text(sender.latest.body, maxLines = 3, overflow = TextOverflow.Ellipsis) },
                            trailingContent = {
                                Column(horizontalAlignment = Alignment.End) {
                                    Text(formatTime(sender.latest.date), style = MaterialTheme.typography.labelSmall)
                                    Text("${sender.count}건", style = MaterialTheme.typography.labelSmall)
                                }
                            },
                        )
                        HorizontalDivider()
                    }
                }
            }
        }
    }
}

/** 규칙 대상(번호/앱)의 최근 문자·알림에서 시험 샘플을 고른다. */
@Composable
fun SamplePickerDialog(rule: Rule, onDismiss: () -> Unit, onPick: (String) -> Unit) {
    val context = LocalContext.current
    when (rule.source) {
        SourceType.SMS -> FullScreenPicker("받은 문자에서 샘플 고르기", onDismiss) {
            RequireReadSms { refresh ->
                val messages by produceState<List<MessageReader.Message>?>(null, refresh, rule.senders) {
                    value = withContext(Dispatchers.IO) {
                        if (rule.senders.isEmpty()) MessageReader.recentMessages(context) else MessageReader.messagesFrom(context, rule.senders)
                    }
                }
                val list = messages
                when {
                    list == null -> Loading()
                    list.isEmpty() -> EmptyText("이 번호에서 받은 문자가 없습니다.")
                    else -> SampleList(list.map { Sample(it.body, it.date, PhoneNumber.format(it.address) + if (it.mms) " · 장문" else "") }, onPick)
                }
            }
        }
        SourceType.NOTIFICATION -> FullScreenPicker("최근 알림에서 샘플 고르기", onDismiss) {
            val recent by RecentNotifications.items.collectAsStateWithLifecycle()
            val list = recent.filter { it.origin == rule.packageName }
            if (list.isEmpty()) {
                EmptyText(
                    "이 앱의 알림을 아직 받지 못했습니다. 알림 접근을 허용한 뒤 알림이 오면 여기에 나타납니다. " +
                        "(최근 알림은 기기 메모리에만 잠깐 두고, 앱을 다시 시작하면 지워집니다)",
                )
            } else {
                SampleList(list.map { Sample(it.fullText, it.receivedAt, null) }, onPick)
            }
        }
    }
}

private data class Sample(val text: String, val time: Long, val label: String?)

@Composable
private fun SampleList(samples: List<Sample>, onPick: (String) -> Unit) {
    LazyColumn {
        items(samples) { sample ->
            ListItem(
                modifier = Modifier.clickable { onPick(sample.text) },
                overlineContent = { Text(listOfNotNull(formatTime(sample.time), sample.label).joinToString(" · ")) },
                headlineContent = { Text(sample.text, maxLines = 6, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodyMedium) },
            )
            HorizontalDivider()
        }
    }
}
