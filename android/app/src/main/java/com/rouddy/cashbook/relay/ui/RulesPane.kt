package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.ParseMode
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.ingest.RuleMatcher
import com.rouddy.cashbook.relay.parse.PhoneNumber
import kotlinx.coroutines.launch

/** 설정 → 규칙: 자동 업로드 스위치와 문자/앱 알림 규칙 목록 */
@Composable
fun RulesPane(contentPadding: PaddingValues, onEdit: (Rule) -> Unit) {
    val scope = rememberCoroutineScope()
    val config by Graph.store.configFlow.collectAsStateWithLifecycle(initialValue = null)
    var deleting by remember { mutableStateOf<Rule?>(null) }
    val current = config ?: return
    val smsRules = current.rules.filter { it.source == SourceType.SMS }
    val notificationRules = current.rules.filter { it.source == SourceType.NOTIFICATION }
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(top = contentPadding.calculateTopPadding(), bottom = contentPadding.calculateBottomPadding() + 88.dp),
    ) {
        item {
            AutoUploadCard(
                enabled = current.settings.enabled,
                onEnabled = { on -> scope.launch { Graph.store.updateSettings { it.copy(enabled = on) } } },
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

/** 규칙 추가 버튼 (문자 규칙 / 앱 알림 규칙) */
@Composable
fun AddRuleButton(onAdd: (SourceType) -> Unit) {
    var addMenu by remember { mutableStateOf(false) }
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
}

@Composable
private fun AutoUploadCard(enabled: Boolean, onEnabled: (Boolean) -> Unit) {
    ElevatedCard(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
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
        cardMappings.size.takeIf { it > 0 && parseMode != ParseMode.SERVER }?.let { "카드별 결제수단 ${it}개" },
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
