package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.rouddy.cashbook.relay.book.MonthlySummary
import com.rouddy.cashbook.relay.book.amountText
import com.rouddy.cashbook.relay.book.compactWon
import com.rouddy.cashbook.relay.book.shortDate
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.pow
import kotlin.math.roundToInt

private class Series(val name: String, val color: Color, val values: List<Double?>, val area: Boolean = false)

/** 눈금 간격: 1·2·2.5·5 × 10^n 중 [raw] 이상인 가장 작은 값 */
internal fun niceStep(raw: Double): Double {
    if (raw <= 0) return 1.0
    val pow = 10.0.pow(floor(log10(raw)))
    return listOf(1.0, 2.0, 2.5, 5.0, 10.0).map { it * pow }.first { it >= raw - 1e-9 }
}

/**
 * 일별 누적 소비 (웹 대시보드 "일별 누적 소비"와 같은 색 규칙): 이번 달 = accent 2dp 선 + 옅은 면,
 * 예정 포함 = 같은 파랑 램프의 밝은 단계, 평균 = 회색. 누르거나 끌면 그날 값이 보이고, 표로도 볼 수 있다.
 * [todayIndex]까지만 이번 달 선을 그린다 (-1이면 그리지 않음).
 */
@Composable
fun DailyChart(s: MonthlySummary, todayIndex: Int, asTable: Boolean, modifier: Modifier = Modifier) {
    val colors = chartColors()
    val days = s.daily
    if (days.isEmpty()) return
    val hasPlanned = s.plannedSpending != 0L
    val series = buildList {
        if (s.averageMonthsUsed > 0) add(Series("${s.averageMonthsUsed}개월 평균", colors.context, days.map { it.averageCumulative }))
        if (hasPlanned) add(Series("예정 포함", colors.planned, days.map { it.projectedCumulative.toDouble() }))
        add(Series("이번 달", colors.accent, days.mapIndexed { i, d -> if (i <= todayIndex) d.cumulative.toDouble() else null }, area = true))
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        // 두 선 이상이면 범례를 항상 둔다 (색만으로 구분하지 않게)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            series.asReversed().forEach { LegendKey(it.name, it.color) }
        }
        if (asTable) DailyTable(s, todayIndex, hasPlanned) else LineChart(s, series, todayIndex)
    }
}

@Composable
private fun LegendKey(name: String, color: Color) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Canvas(Modifier.width(16.dp).height(8.dp)) {
            drawLine(color, Offset(0f, size.height / 2), Offset(size.width, size.height / 2), strokeWidth = 2.dp.toPx(), cap = StrokeCap.Round)
        }
        Spacer(Modifier.width(6.dp))
        Text(name, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun LineChart(s: MonthlySummary, series: List<Series>, todayIndex: Int) {
    val colors = chartColors()
    val days = s.daily
    val n = days.size
    val measurer = rememberTextMeasurer()
    val axisStyle = MaterialTheme.typography.labelSmall.copy(color = MaterialTheme.colorScheme.onSurfaceVariant)
    val tipStyle = MaterialTheme.typography.labelMedium.copy(color = MaterialTheme.colorScheme.inverseOnSurface)
    val tipBg = MaterialTheme.colorScheme.inverseSurface
    val ring = MaterialTheme.colorScheme.surfaceContainerLow
    var selected by remember(s.month, n) { mutableStateOf<Int?>(null) }

    val maxValue = series.flatMap { it.values }.filterNotNull().maxOrNull() ?: 0.0
    val step = niceStep(maxValue / 3)
    val yMax = (ceil(maxValue / step) * step).coerceAtLeast(step)
    val gridValues = generateSequence(0.0) { it + step }.takeWhile { it <= yMax + 1e-6 }.toList()
    val labelWidth = gridValues.maxOf { measurer.measure(compactWon(it.toLong()), axisStyle).size.width }
    val current = days.getOrNull(todayIndex)

    Box(
        Modifier
            .fillMaxWidth()
            .height(190.dp)
            .semantics {
                contentDescription = "일별 누적 소비 차트. " + (current?.let { "${it.day}일까지 ${amountText(it.cumulative)}원. " } ?: "") +
                    "표로 보기에서 날짜별 값을 볼 수 있습니다"
            },
    ) {
        Canvas(
            Modifier
                .fillMaxWidth()
                .height(190.dp)
                .pointerInput(n, labelWidth) {
                    val left = labelWidth + 8.dp.toPx()
                    fun index(x: Float) = (((x - left) / (size.width - left)) * (n - 1)).roundToInt().coerceIn(0, n - 1)
                    detectTapGestures { offset -> selected = index(offset.x).takeIf { it != selected } }
                }
                .pointerInput(n, labelWidth) {
                    val left = labelWidth + 8.dp.toPx()
                    fun index(x: Float) = (((x - left) / (size.width - left)) * (n - 1)).roundToInt().coerceIn(0, n - 1)
                    detectHorizontalDragGestures { change, _ -> selected = index(change.position.x) }
                },
        ) {
            val left = labelWidth + 8.dp.toPx()
            val bottomAxis = 20.dp.toPx()
            val top = 8.dp.toPx()
            val plotW = size.width - left
            val plotH = size.height - bottomAxis - top
            fun x(i: Int) = left + if (n > 1) plotW * i / (n - 1) else 0f
            fun y(v: Double) = top + plotH * (1 - (v / yMax).toFloat())

            // 눈금선: 표면에서 한 단계 떨어진 회색 1px 실선
            gridValues.forEach { g ->
                drawLine(colors.grid, Offset(left, y(g)), Offset(size.width, y(g)), strokeWidth = 1f)
                val label = measurer.measure(compactWon(g.toLong()), axisStyle)
                drawText(label, topLeft = Offset(labelWidth - label.size.width.toFloat(), y(g) - label.size.height / 2f))
            }
            listOf(0, 9, 19, n - 1).filter { it in 0 until n }.distinct().forEach { i ->
                val label = measurer.measure("${days[i].day}일", axisStyle)
                val lx = (x(i) - label.size.width / 2f).coerceIn(left, size.width - label.size.width)
                drawText(label, topLeft = Offset(lx, size.height - label.size.height))
            }

            series.forEach { line ->
                val points = line.values.mapIndexedNotNull { i, v -> v?.let { Offset(x(i), y(it)) } }
                if (points.isEmpty()) return@forEach
                val path = Path().apply {
                    moveTo(points[0].x, points[0].y)
                    points.drop(1).forEach { lineTo(it.x, it.y) }
                }
                if (line.area && points.size > 1) {
                    val area = Path().apply {
                        addPath(path)
                        lineTo(points.last().x, y(0.0))
                        lineTo(points.first().x, y(0.0))
                        close()
                    }
                    drawPath(area, line.color.copy(alpha = 0.10f))
                }
                drawPath(path, line.color, style = Stroke(width = 2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
            // 이번 달 선 끝점
            current?.let { dot(Offset(x(todayIndex), y(it.cumulative.toDouble())), colors.accent, ring) }

            selected?.let { i -> drawSelection(i, series, days[i].date, ::x, ::y, top, plotH, measurer, tipStyle, tipBg, ring, colors.context) }
        }
    }
}

private fun DrawScope.dot(at: Offset, color: Color, ring: Color) {
    drawCircle(ring, radius = 6.dp.toPx(), center = at)
    drawCircle(color, radius = 4.dp.toPx(), center = at)
}

private fun DrawScope.drawSelection(
    i: Int,
    series: List<Series>,
    date: String,
    x: (Int) -> Float,
    y: (Double) -> Float,
    top: Float,
    plotH: Float,
    measurer: TextMeasurer,
    style: TextStyle,
    bg: Color,
    ring: Color,
    crosshair: Color,
) {
    val cx = x(i)
    drawLine(crosshair, Offset(cx, top), Offset(cx, top + plotH), strokeWidth = 1.dp.toPx())
    val rows = series.mapNotNull { s -> s.values[i]?.let { s to it } }
    rows.forEach { (s, v) -> dot(Offset(cx, y(v)), s.color, ring) }
    val text = buildString {
        append(shortDate(date))
        rows.asReversed().forEach { (s, v) -> append("\n${s.name}  ${amountText(v)}원") }
    }
    val layout = measurer.measure(text, style.copy(textAlign = TextAlign.Start))
    val pad = 8.dp.toPx()
    val w = layout.size.width + pad * 2
    val h = layout.size.height + pad * 2
    val left = if (cx + 12.dp.toPx() + w <= size.width) cx + 12.dp.toPx() else (cx - 12.dp.toPx() - w).coerceAtLeast(0f)
    drawRoundRect(bg, topLeft = Offset(left, top), size = Size(w, h), cornerRadius = CornerRadius(6.dp.toPx()))
    drawText(layout, topLeft = Offset(left + pad, top + pad))
}

@Composable
private fun DailyTable(s: MonthlySummary, todayIndex: Int, hasPlanned: Boolean) {
    val header = MaterialTheme.typography.labelMedium
    val body = MaterialTheme.typography.bodySmall
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    Column {
        Row(Modifier.padding(vertical = 4.dp)) {
            Text("날짜", Modifier.weight(1.1f), style = header, color = muted)
            Text("당일", Modifier.weight(1f), style = header, color = muted, textAlign = TextAlign.End)
            Text("누적", Modifier.weight(1.2f), style = header, color = muted, textAlign = TextAlign.End)
            if (hasPlanned) Text("예정 포함", Modifier.weight(1.2f), style = header, color = muted, textAlign = TextAlign.End)
            Text("평균 누적", Modifier.weight(1.2f), style = header, color = muted, textAlign = TextAlign.End)
        }
        HorizontalDivider()
        s.daily.forEachIndexed { i, d ->
            Row(Modifier.padding(vertical = 3.dp)) {
                Text(shortDate(d.date), Modifier.weight(1.1f), style = body)
                Text(amountText(d.spending), Modifier.weight(1f), style = body, textAlign = TextAlign.End)
                Text(if (i <= todayIndex) amountText(d.cumulative) else "-", Modifier.weight(1.2f), style = body, textAlign = TextAlign.End)
                if (hasPlanned) Text(amountText(d.projectedCumulative), Modifier.weight(1.2f), style = body, textAlign = TextAlign.End)
                Text(amountText(d.averageCumulative), Modifier.weight(1.2f), style = body, color = muted, textAlign = TextAlign.End)
            }
        }
    }
}
