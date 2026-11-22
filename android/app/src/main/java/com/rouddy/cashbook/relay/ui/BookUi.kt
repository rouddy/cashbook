package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.rouddy.cashbook.relay.book.BookException

/**
 * 차트 색 (웹 `styles.css`의 dataviz 토큰과 같은 값). 기기 테마 색(Material You)과 관계없이
 * 이번 달 = accent 파랑, 예정 = 같은 램프의 밝은 단계, 비교 맥락 = 회색으로 고정한다.
 */
@Immutable
data class ChartColors(
    val accent: Color,
    /** 확정분 뒤에 쌓는 예정분 */
    val planned: Color,
    /** 막대 트랙 (accent 램프의 옅은 단계) */
    val track: Color,
    /** 평균 등 비교 맥락 */
    val context: Color,
    val grid: Color,
    /** 한도 초과 막대 */
    val badMark: Color,
    /** 글자 색: 늘면 좋은 값/나쁜 값 */
    val goodText: Color,
    val badText: Color,
)

private val LightChart = ChartColors(
    accent = Color(0xFF2A78D6),
    planned = Color(0xFF6DA7EC),
    track = Color(0xFFE6F0FC),
    context = Color(0xFF898781),
    grid = Color(0xFFE1E0D9),
    badMark = Color(0xFFD03B3B),
    goodText = Color(0xFF006300),
    badText = Color(0xFFC22F2F),
)

private val DarkChart = ChartColors(
    accent = Color(0xFF3987E5),
    planned = Color(0xFF1C5CAB),
    track = Color(0xFF1B2A3D),
    context = Color(0xFF898781),
    grid = Color(0xFF2C2C2A),
    badMark = Color(0xFFD03B3B),
    goodText = Color(0xFF0CA30C),
    badText = Color(0xFFE66767),
)

@Composable
fun chartColors(): ChartColors = if (isSystemInDarkTheme()) DarkChart else LightChart

/**
 * 막대 하나 (웹 `Meter`). [goal]이면 목표형(넘으면 달성), 아니면 한도형(확정+예정이 [max]를 넘으면 경고색).
 * [planned]는 확정분 끝에서 2dp 띄워 밝은 파랑으로 쌓는다. [ticks]는 막대 위 구분선(실적 구간, 평균 등).
 * [track]이 false면 트랙 없이 길이만 비교하는 막대(카테고리 비교)로 그린다.
 */
@Composable
fun Meter(
    value: Long,
    max: Long,
    description: String,
    modifier: Modifier = Modifier,
    planned: Long = 0,
    ticks: List<Double> = emptyList(),
    goal: Boolean = false,
    track: Boolean = true,
) {
    val colors = chartColors()
    val tickColor = MaterialTheme.colorScheme.onSurfaceVariant
    val over = !goal && max > 0 && value + planned.coerceAtLeast(0) > max
    Canvas(
        modifier
            .fillMaxWidth()
            .height(8.dp)
            .semantics { contentDescription = description },
    ) {
        val w = size.width
        val h = size.height
        val radius = CornerRadius(h / 2, h / 2)
        fun ratio(v: Double) = if (max > 0) (v / max).coerceIn(0.0, 1.0).toFloat() else 0f
        if (track) drawRoundRect(colors.track, cornerRadius = radius)
        val confirmed = ratio(value.toDouble()) * w
        if (confirmed > 0f) drawRoundRect(if (over) colors.badMark else colors.accent, size = Size(confirmed, h), cornerRadius = radius)
        val plannedEnd = ratio(value.toDouble() + planned.coerceAtLeast(0)) * w
        val gap = if (confirmed > 0f) 2.dp.toPx() else 0f
        if (plannedEnd - confirmed - gap > 0f) {
            drawRoundRect(
                colors.planned,
                topLeft = Offset(confirmed + gap, 0f),
                size = Size(plannedEnd - confirmed - gap, h),
                cornerRadius = radius,
            )
        }
        val tickW = 2.dp.toPx()
        ticks.filter { it >= 0 && max > 0 }.forEach { t ->
            val x = (ratio(t) * w - tickW / 2).coerceIn(0f, w - tickW)
            drawRect(tickColor, topLeft = Offset(x, -2.dp.toPx()), size = Size(tickW, h + 4.dp.toPx()))
        }
    }
}

/** 대시보드 카드 하나: 제목 · 부제 · 내용 */
@Composable
fun Panel(
    title: String,
    modifier: Modifier = Modifier,
    sub: String? = null,
    action: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    ElevatedCard(modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(title, style = MaterialTheme.typography.titleMedium)
                    sub?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                }
                action?.invoke()
            }
            content()
        }
    }
}

/** 숫자 하나를 크게 보이는 칸 (웹 `StatTile`) */
@Composable
fun StatTile(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
    valueColor: Color = Color.Unspecified,
    delta: (@Composable () -> Unit)? = null,
    extra: (@Composable () -> Unit)? = null,
) {
    ElevatedCard(modifier) {
        Column(Modifier.fillMaxWidth().padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(value, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold, color = valueColor, maxLines = 1)
            delta?.let {
                CompositionLocalProvider(
                    LocalTextStyle provides MaterialTheme.typography.bodySmall,
                    LocalContentColor provides MaterialTheme.colorScheme.onSurfaceVariant,
                ) { it() }
            }
            extra?.let {
                Spacer(Modifier.height(4.dp))
                it()
            }
        }
    }
}

/** 가계부 서버 오류: 메시지와 다시 시도 / 서버 설정 열기 */
@Composable
fun BookErrorCard(error: BookException, onRetry: () -> Unit, onOpenSettings: () -> Unit, modifier: Modifier = Modifier) {
    ElevatedCard(modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                Icon(
                    Icons.Default.Warning,
                    contentDescription = null,
                    tint = if (error.fixInSettings) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                )
                Spacer(Modifier.width(12.dp))
                Text(error.message.orEmpty(), style = MaterialTheme.typography.bodyMedium)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (error.fixInSettings) Button(onClick = onOpenSettings) { Text("서버 설정 열기") }
                OutlinedButton(onClick = onRetry) { Text("다시 시도") }
            }
        }
    }
}
