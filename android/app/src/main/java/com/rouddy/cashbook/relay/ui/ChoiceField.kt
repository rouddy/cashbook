package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

/**
 * 드롭다운 항목. [value]는 규칙에 저장해 서버로 보낼 값(가계부의 이름).
 * [group]이 바뀌는 곳에 구분 제목을 넣는다. [tag]는 부르는 쪽이 쓰는 값(카테고리의 장부 등).
 */
data class Choice(
    val value: String,
    val label: String = value,
    val detail: String? = null,
    val group: String? = null,
    val tag: String? = null,
)

/** 서버가 이름을 맞춰 보는 방식과 같게 공백·대소문자를 무시하고 비교 */
fun sameName(a: String?, b: String?): Boolean {
    fun squash(s: String) = s.filterNot(Char::isWhitespace).lowercase()
    return a != null && b != null && squash(a) == squash(b)
}

/**
 * 가계부에서 받은 목록에서 고르는 칸. [choices]가 null이면(서버 미설정·연결 실패) 직접 입력한다.
 * [value]가 있는데 [selected]가 없으면 가계부에 없는 값으로 표시한다.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChoiceField(
    label: String,
    value: String,
    choices: List<Choice>?,
    emptyLabel: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    selected: Choice? = choices?.firstOrNull { sameName(it.value, value) },
    onPick: (Choice?) -> Unit = { onValueChange(it?.value.orEmpty()) },
    supportingText: String? = null,
    /** 비우기 항목을 둘지 (비우면 서버가 찾는 칸) */
    allowEmpty: Boolean = true,
    /** 좁은 칸에서는 끄면 메뉴가 항목 길이에 맞춰 넓어진다 */
    matchAnchorWidth: Boolean = true,
) {
    if (choices == null) {
        OutlinedTextField(
            value = value,
            onValueChange = onValueChange,
            label = { Text(label) },
            placeholder = { Text(emptyLabel, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            supportingText = supportingText?.let { { Text(it) } },
            singleLine = true,
            modifier = modifier,
        )
        return
    }
    var expanded by remember { mutableStateOf(false) }
    val missing = value.isNotBlank() && selected == null
    val hint = MaterialTheme.colorScheme.onSurfaceVariant
    ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { expanded = it }, modifier = modifier) {
        OutlinedTextField(
            // 비었을 때도 무엇을 뜻하는지 보이게 빈 항목 이름을 값 자리에 둔다
            value = selected?.label ?: value.ifBlank { emptyLabel },
            onValueChange = {},
            readOnly = true,
            singleLine = true,
            label = { Text(label) },
            textStyle = if (value.isBlank()) {
                MaterialTheme.typography.bodyLarge.copy(color = hint)
            } else {
                MaterialTheme.typography.bodyLarge
            },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
            isError = missing,
            supportingText = (if (missing) "가계부에 없습니다 (이름이 바뀌었거나 보관됨)" else supportingText)?.let { { Text(it) } },
            modifier = Modifier
                .menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable)
                .fillMaxWidth(),
        )
        ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }, matchAnchorWidth = matchAnchorWidth) {
            if (allowEmpty) {
                DropdownMenuItem(
                    text = { Text(emptyLabel, color = hint) },
                    trailingIcon = if (value.isBlank()) ({ Icon(Icons.Default.Check, contentDescription = "선택됨") }) else null,
                    onClick = {
                        expanded = false
                        onPick(null)
                    },
                )
            }
            if (choices.isEmpty()) {
                Text(
                    "가계부에 항목이 없습니다",
                    style = MaterialTheme.typography.bodySmall,
                    color = hint,
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
            choices.forEachIndexed { i, choice ->
                if (choice.group != null && choice.group != choices.getOrNull(i - 1)?.group) {
                    Text(
                        choice.group,
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.padding(start = 12.dp, end = 12.dp, top = 8.dp, bottom = 2.dp),
                    )
                }
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(choice.label)
                            choice.detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = hint) }
                        }
                    },
                    trailingIcon = if (choice == selected) ({ Icon(Icons.Default.Check, contentDescription = "선택됨") }) else null,
                    onClick = {
                        expanded = false
                        onPick(choice)
                    },
                )
            }
        }
    }
}
