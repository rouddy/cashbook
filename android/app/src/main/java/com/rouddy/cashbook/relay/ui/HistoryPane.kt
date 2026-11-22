package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.AppJson
import com.rouddy.cashbook.relay.data.UploadRecord
import com.rouddy.cashbook.relay.data.UploadStatus
import com.rouddy.cashbook.relay.ingest.Ingestor
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement

@Composable
private fun statusLabel(status: UploadStatus): Pair<String, Color> = when (status) {
    UploadStatus.PENDING -> "대기" to MaterialTheme.colorScheme.tertiary
    UploadStatus.SUCCESS -> "입력됨" to MaterialTheme.colorScheme.primary
    UploadStatus.DUPLICATE -> "중복" to MaterialTheme.colorScheme.outline
    UploadStatus.FAILED -> "실패" to MaterialTheme.colorScheme.error
    UploadStatus.SKIPPED -> "건너뜀" to MaterialTheme.colorScheme.outline
}

private fun UploadRecord.headline(): String {
    val r = result
    return if (r != null && r.amount != null) {
        listOfNotNull(formatWon(r.amount), r.description).joinToString(" ")
    } else {
        text.lineSequence().map(String::trim).firstOrNull { it.isNotEmpty() && it != "[Web발신]" } ?: text
    }
}

private fun UploadRecord.detailLine(): String {
    val r = result
    val parts = when {
        r != null -> listOfNotNull(r.category, r.paymentMethod, if (r.matchedPlanned) "예정 거래 확정" else null) +
            r.warnings.map { "⚠ $it" }
        message != null -> listOf(message)
        else -> emptyList()
    }
    return (listOf(ruleName.ifBlank { originLabel }) + parts).joinToString(" · ")
}

/** 설정 → 업로드 기록 상단 메뉴: 실패한 항목 다시 보내기, 기록 지우기 */
@Composable
fun HistoryMenu(snackbar: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val records by Graph.store.recordsFlow.collectAsStateWithLifecycle(initialValue = emptyList())
    var menu by remember { mutableStateOf(false) }
    var confirmClear by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, contentDescription = "기록 메뉴") }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            DropdownMenuItem(
                text = { Text("실패한 항목 다시 보내기") },
                enabled = records.any { it.status == UploadStatus.FAILED },
                onClick = {
                    menu = false
                    scope.launch {
                        val n = Ingestor.resendFailed(context)
                        snackbar.showSnackbar("${n}건을 다시 보냅니다")
                    }
                },
            )
            DropdownMenuItem(
                text = { Text("기록 지우기") },
                enabled = records.isNotEmpty(),
                onClick = {
                    menu = false
                    confirmClear = true
                },
            )
        }
    }
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text("기록 지우기") },
            text = { Text("보내기를 기다리는 항목을 빼고 모두 지웁니다. 지운 문자가 다시 오면 다시 보냅니다.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    scope.launch { Graph.store.clearFinishedRecords() }
                }) { Text("지우기") }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text("취소") } },
        )
    }
}

/** 설정 → 업로드 기록: 받은 문자·알림과 서버로 보낸 결과 */
@Composable
fun HistoryPane(contentPadding: PaddingValues) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val records by Graph.store.recordsFlow.collectAsStateWithLifecycle(initialValue = emptyList())
    var selected by remember { mutableStateOf<String?>(null) }

    if (records.isEmpty()) {
        Text(
            "아직 받은 내역이 없습니다. 규칙에 맞는 문자·알림을 받으면 여기에 쌓입니다.",
            Modifier.padding(contentPadding).padding(24.dp),
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    } else {
        LazyColumn(Modifier.fillMaxSize(), contentPadding = contentPadding) {
            items(records, key = { it.id }) { record ->
                val (label, color) = statusLabel(record.status)
                ListItem(
                    modifier = Modifier.clickable { selected = record.id },
                    leadingContent = { SourceIcon(record.source, record.origin, size = 32.dp) },
                    overlineContent = {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text(label, color = color)
                            Text(formatTime(record.receivedAt))
                            Text(record.originLabel, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    },
                    headlineContent = { Text(record.headline(), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    supportingContent = { Text(record.detailLine(), maxLines = 2, overflow = TextOverflow.Ellipsis) },
                )
                HorizontalDivider()
            }
        }
    }

    selected?.let { id -> records.firstOrNull { it.id == id } }?.let { record ->
        RecordDialog(
            record = record,
            onDismiss = { selected = null },
            onResend = {
                scope.launch { Ingestor.resend(context, record.id) }
                selected = null
            },
            onDelete = {
                scope.launch { Graph.store.deleteRecord(record.id) }
                selected = null
            },
        )
    }
}

private val prettyJson = Json { prettyPrint = true }

private fun pretty(json: String): String = runCatching {
    prettyJson.encodeToString(JsonElement.serializer(), AppJson.parseToJsonElement(json))
}.getOrDefault(json)

@Composable
private fun RecordDialog(record: UploadRecord, onDismiss: () -> Unit, onResend: () -> Unit, onDelete: () -> Unit) {
    val (label, color) = statusLabel(record.status)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(label, color = color) },
        text = {
            SelectionContainer {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Field("받은 시각", formatFullTime(record.receivedAt))
                    Field("출처", "${record.originLabel} (${record.origin})")
                    Field("규칙", record.ruleName.ifBlank { "(이름 없음)" })
                    record.message?.let { Field("메시지", it) }
                    record.result?.let { r ->
                        Field(
                            "입력된 거래",
                            listOfNotNull(
                                r.transactionId?.let { "#$it" },
                                r.date,
                                r.amount?.let(::formatWon),
                                r.description,
                                r.ledger,
                                r.category,
                                r.paymentMethod,
                                if (r.matchedPlanned) "예정 거래를 확정함" else null,
                            ).joinToString(" · "),
                        )
                        if (r.warnings.isNotEmpty()) Field("경고", r.warnings.joinToString("\n"))
                    }
                    if (record.attempts > 0) Field("시도", "${record.attempts}번")
                    Field("원문", record.text, mono = true)
                    record.payload?.let { Field("보낸 내용", pretty(it), mono = true) }
                }
            }
        },
        confirmButton = {
            when (record.status) {
                UploadStatus.FAILED -> TextButton(onClick = onResend) { Text("다시 보내기") }
                UploadStatus.SKIPPED -> TextButton(onClick = onResend) { Text("원문 보내기") }
                else -> TextButton(onClick = onDismiss) { Text("닫기") }
            }
        },
        dismissButton = {
            Row {
                if (record.status != UploadStatus.PENDING) TextButton(onClick = onDelete) { Text("삭제") }
                if (record.status == UploadStatus.FAILED || record.status == UploadStatus.SKIPPED) {
                    TextButton(onClick = onDismiss) { Text("닫기") }
                }
            }
        },
    )
}

@Composable
private fun Field(label: String, value: String, mono: Boolean = false) {
    Column {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Surface(color = if (mono) MaterialTheme.colorScheme.surfaceVariant else Color.Transparent, shape = MaterialTheme.shapes.small) {
            Text(
                value,
                style = if (mono) MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace) else MaterialTheme.typography.bodyMedium,
                modifier = if (mono) Modifier.padding(8.dp) else Modifier,
            )
        }
    }
}
