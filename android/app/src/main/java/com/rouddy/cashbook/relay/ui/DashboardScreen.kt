package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.book.BookException
import com.rouddy.cashbook.relay.book.CardReport
import com.rouddy.cashbook.relay.book.Dashboard
import com.rouddy.cashbook.relay.book.Funds
import com.rouddy.cashbook.relay.book.MonthlySummary
import com.rouddy.cashbook.relay.book.RefData
import com.rouddy.cashbook.relay.book.Tx
import com.rouddy.cashbook.relay.book.amountText
import com.rouddy.cashbook.relay.book.benefitTotalsText
import com.rouddy.cashbook.relay.book.isInflow
import com.rouddy.cashbook.relay.book.monthTitle
import com.rouddy.cashbook.relay.book.parseDate
import com.rouddy.cashbook.relay.book.shortDate
import com.rouddy.cashbook.relay.book.signedWon
import com.rouddy.cashbook.relay.book.txAmountText
import com.rouddy.cashbook.relay.book.txDetail
import com.rouddy.cashbook.relay.data.UiPrefs
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.YearMonth
import kotlin.math.abs
import kotlin.math.roundToLong

class DashboardViewModel : ViewModel() {
    var month by mutableStateOf(YearMonth.now())
        private set

    /** [UiPrefs.LEDGER_DEFAULT], [UiPrefs.LEDGER_ALL] 또는 장부 id */
    var ledgerChoice by mutableStateOf(Graph.prefs.dashboardLedger)
        private set
    var ref by mutableStateOf(Graph.book.ref.value)
        private set
    var data by mutableStateOf<Dashboard?>(null)
        private set

    /** 지금 보이는 [data]를 만든 조건 (다른 달을 고른 직후 이전 달 값을 흐리게 보이기 위해) */
    var dataKey by mutableStateOf<Pair<YearMonth, Int?>?>(null)
        private set
    var error by mutableStateOf<BookException?>(null)
        private set
    var loading by mutableStateOf(false)
        private set
    var refreshing by mutableStateOf(false)
        private set

    /** 확정/건너뛰기 중인 예정 거래 */
    var busyTx by mutableStateOf<Int?>(null)
        private set
    var message by mutableStateOf<String?>(null)

    private var generation = 0
    private var visible = false

    init {
        // 수동 입력·자동 입력으로 거래가 바뀌면, 보고 있을 때 바로 다시 읽는다 (안 보일 때는 다시 열 때 읽음)
        viewModelScope.launch { Graph.book.changes.drop(1).collect { if (visible) load(refreshRef = false) } }
    }

    /** 고른 장부 id (null = 전체 장부) */
    val ledgerId: Int?
        get() = when (ledgerChoice) {
            UiPrefs.LEDGER_ALL -> null
            UiPrefs.LEDGER_DEFAULT -> ref?.defaultLedger?.id
            else -> ledgerChoice.toIntOrNull()?.takeIf { ref?.ledger(it) != null } ?: ref?.defaultLedger?.id
        }

    fun onVisible(isVisible: Boolean) {
        visible = isVisible
        if (isVisible) load()
    }

    fun refresh() = load(user = true)

    fun shiftMonth(delta: Long) {
        month = month.plusMonths(delta)
        load(refreshRef = false)
    }

    fun thisMonth() {
        if (month != YearMonth.now()) {
            month = YearMonth.now()
            load(refreshRef = false)
        }
    }

    fun chooseLedger(choice: String) {
        ledgerChoice = choice
        Graph.prefs.dashboardLedger = choice
        load(refreshRef = false)
    }

    fun load(user: Boolean = false, refreshRef: Boolean = true) {
        val gen = ++generation
        loading = true
        if (user) refreshing = true
        viewModelScope.launch {
            try {
                val r = if (refreshRef || ref == null) Graph.book.loadRef() else ref!!
                if (gen != generation) return@launch
                ref = r
                val key = month to ledgerId
                val result = Graph.book.dashboard(key.first, key.second, LocalDate.now())
                if (gen != generation) return@launch
                data = result
                dataKey = key
                error = null
            } catch (e: BookException) {
                if (gen == generation) {
                    error = e
                    // 다른 서버·토큰으로 바뀌었으면 이전 값을 보이지 않는다
                    if (e.fixInSettings) {
                        data = null
                        dataKey = null
                    }
                }
            } finally {
                if (gen == generation) {
                    loading = false
                    refreshing = false
                }
            }
        }
    }

    fun confirm(tx: Tx) = act(tx, "확정했습니다") { Graph.book.confirm(it.id) }

    fun skip(tx: Tx) = act(tx, "이번 달은 건너뜁니다") { Graph.book.skip(it.id) }

    private fun act(tx: Tx, done: String, block: suspend (Tx) -> Unit) {
        if (busyTx != null) return
        busyTx = tx.id
        viewModelScope.launch {
            try {
                block(tx)
                message = "${tx.description} — $done"
            } catch (e: BookException) {
                message = e.message
            } finally {
                busyTx = null
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DashboardScreen(onOpenSettings: () -> Unit) {
    val vm: DashboardViewModel = viewModel()
    val snackbar = remember { SnackbarHostState() }
    LifecycleResumeEffect(Unit) {
        vm.onVisible(true)
        onPauseOrDispose { vm.onVisible(false) }
    }
    val scope = rememberCoroutineScope()
    // 메시지를 비우면 이 효과가 다시 시작되므로, 스낵바는 화면 범위에서 띄워 취소되지 않게 한다
    LaunchedEffect(vm.message) {
        vm.message?.let { text ->
            vm.message = null
            scope.launch { snackbar.showSnackbar(text) }
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("대시보드") },
                actions = { IconButton(onClick = vm::refresh, enabled = !vm.loading) { Icon(Icons.Default.Refresh, contentDescription = "새로 고침") } },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        PullToRefreshBox(isRefreshing = vm.refreshing, onRefresh = vm::refresh, modifier = Modifier.fillMaxSize().padding(padding)) {
            val ref = vm.ref
            val data = vm.data
            val stale = vm.dataKey != (vm.month to vm.ledgerId)
            LazyColumn(
                Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(key = "filters") {
                    Filters(vm, ref)
                    if (vm.loading && !vm.refreshing) LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 4.dp))
                }
                vm.error?.let { e ->
                    item(key = "error") { BookErrorCard(e, onRetry = vm::refresh, onOpenSettings = onOpenSettings) }
                }
                if (data != null && ref != null) {
                    // 다른 달·장부를 고른 직후에는 새 값이 올 때까지 이전 값을 흐리게 둔다
                    val dim = if (stale) Modifier.alpha(0.45f) else Modifier
                    val s = data.monthlySummary
                    item(key = "kpis") { Kpis(s, dim) }
                    item(key = "daily") { DailyPanel(s, dim) }
                    item(key = "cards") { CardsPanel(data.cardsMonthly, ref, YearMonth.parse(s.month), dim) }
                    data.availableFunds?.let { funds -> item(key = "funds") { FundsPanel(funds, ref, dim) } }
                    item(key = "planned") { PlannedPanel(data.planned.items, ref, vm.ledgerId, vm.busyTx, vm::confirm, vm::skip, dim) }
                    item(key = "categories") { CategoryPanel(s, ref, vm.ledgerId, dim) }
                }
            }
        }
    }
}

@Composable
private fun Filters(vm: DashboardViewModel, ref: RefData?) {
    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { vm.shiftMonth(-1) }) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = "이전 달") }
            TextButton(onClick = vm::thisMonth) {
                Text(monthTitle(vm.month), style = MaterialTheme.typography.titleMedium)
            }
            IconButton(onClick = { vm.shiftMonth(1) }) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = "다음 달") }
            Spacer(Modifier.weight(1f))
            if (vm.month != YearMonth.now()) TextButton(onClick = vm::thisMonth) { Text("이번 달") }
        }
        if (ref != null && ref.ledgers.isNotEmpty()) {
            val selected = vm.ledgerId
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ref.ledgers.forEach { l ->
                    FilterChip(
                        selected = selected == l.id,
                        onClick = { vm.chooseLedger(if (l.isDefault) UiPrefs.LEDGER_DEFAULT else l.id.toString()) },
                        label = { Text(l.name) },
                    )
                }
                FilterChip(selected = selected == null, onClick = { vm.chooseLedger(UiPrefs.LEDGER_ALL) }, label = { Text("전체 장부") })
            }
        }
    }
}

@Composable
private fun Kpis(s: MonthlySummary, modifier: Modifier = Modifier) {
    val colors = chartColors()
    val m = YearMonth.parse(s.month).monthValue
    Column(modifier, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            StatTile(
                label = "${m}월 소비",
                value = "${amountText(s.spending)}원",
                modifier = Modifier.weight(1f),
                delta = {
                    if (s.averageMonthsUsed > 0) {
                        val diff = (s.spending - s.averageSpending).roundToLong()
                        Column {
                            when {
                                diff > 0 -> Text("▲ ${amountText(diff)} 평균보다 많음", color = colors.badText)
                                diff < 0 -> Text("▼ ${amountText(abs(diff))} 평균보다 적음", color = colors.goodText)
                                else -> Text("평균과 같음")
                            }
                            Text("${s.averageMonthsUsed}개월 평균 ${amountText(s.averageSpending)}원")
                        }
                    } else {
                        Text("비교할 지난 데이터 없음")
                    }
                },
            )
            StatTile(
                label = "${m}월 수입",
                value = "${amountText(s.income)}원",
                modifier = Modifier.weight(1f),
                delta = {
                    Column {
                        Text(if (s.plannedIncome != 0L) "예정 ${signedWon(s.plannedIncome)}" else "수입 카테고리 합계")
                        if (s.transferIn != 0L || s.plannedTransferIn != 0L) {
                            Text("받은 이체 ${signedWon(s.transferIn)}" + if (s.plannedTransferIn != 0L) " (예정 ${amountText(s.plannedTransferIn)})" else "")
                        }
                    }
                },
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            val balance = s.balance
            StatTile(
                label = "월잔액",
                value = balance?.let { "${amountText(it)}원" } ?: "-",
                modifier = Modifier.weight(1f),
                valueColor = if ((balance ?: 0) < 0) colors.badText else Color.Unspecified,
                delta = {
                    Text(
                        if (balance != null) {
                            "예정(월급·이체 등) 반영 시 ${amountText(s.projectedBalance ?: balance)}원"
                        } else {
                            "장부를 고르면 수입 − 소비 − 이체·배분으로 계산합니다"
                        },
                    )
                },
            )
            val projected = s.spending + s.plannedSpending
            val budget = s.budgetTotal
            StatTile(
                label = "예정 포함 예상 소비",
                value = "${amountText(projected)}원",
                modifier = Modifier.weight(1f),
                delta = {
                    Text(
                        if (budget != null && budget > 0) {
                            "예산 ${amountText(budget)}원의 ${(projected * 100.0 / budget).roundToLong()}%"
                        } else {
                            "남은 예정 ${amountText(s.plannedSpending)}원"
                        },
                    )
                },
                extra = if (budget != null && budget > 0) {
                    {
                        Meter(
                            value = s.spending,
                            planned = s.plannedSpending,
                            max = budget,
                            description = "예산 ${amountText(budget)}원 중 확정 ${amountText(s.spending)}원, 예정 ${amountText(s.plannedSpending)}원",
                        )
                    }
                } else {
                    null
                },
            )
        }
    }
}

@Composable
private fun DailyPanel(s: MonthlySummary, modifier: Modifier = Modifier) {
    var asTable by rememberSaveable { mutableStateOf(false) }
    val month = YearMonth.parse(s.month)
    val now = YearMonth.now()
    val todayIndex = when {
        month == now -> LocalDate.now().dayOfMonth - 1
        month.isAfter(now) -> -1
        else -> s.daily.lastIndex
    }
    Panel(
        modifier = modifier,
        title = "일별 누적 소비",
        sub = "소비 합계 기준 (수입·장부 이체 제외) · 누르거나 끌어서 날짜별 값 보기",
        action = { TextButton(onClick = { asTable = !asTable }) { Text(if (asTable) "차트로 보기" else "표로 보기") } },
    ) {
        DailyChart(s, todayIndex, asTable)
    }
}

@Composable
private fun CardsPanel(cards: List<CardReport>, ref: RefData, month: YearMonth, modifier: Modifier = Modifier) {
    val rows = cards.filter { it.worthShowing }
    Panel(modifier = modifier, title = "카드 실적 현황", sub = "${monthTitle(month)} · 실적이 목표에 닿아야 다음 달 혜택을 받습니다") {
        if (rows.isEmpty()) {
            EmptyText("카드 사용 내역이 없습니다")
            return@Panel
        }
        rows.forEachIndexed { i, r ->
            if (i > 0) HorizontalDivider()
            CardRow(r, ref.paymentMethod(r.paymentMethodId)?.name ?: "카드 #${r.paymentMethodId}")
        }
    }
}

@Composable
private fun CardRow(r: CardReport, name: String) {
    val colors = chartColors()
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val planned = r.plannedPerformance
    Column(Modifier.padding(vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(name, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            val target = r.target
            when {
                target == null -> Text("목표 없음", style = MaterialTheme.typography.bodySmall, color = muted)
                (r.remainingToTarget ?: 0) > 0 -> Text("목표까지 ${amountText(r.remainingToTarget ?: 0)}원", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
                else -> Text("✓ 달성", style = MaterialTheme.typography.bodyMedium, color = colors.goodText, fontWeight = FontWeight.SemiBold)
            }
        }
        val steps = r.performanceSteps
        steps.lastOrNull()?.let { max ->
            Meter(
                value = r.performance,
                planned = planned.coerceAtLeast(0),
                max = max,
                ticks = steps.dropLast(1).map { it.toDouble() },
                goal = true,
                description = "$name 실적 ${amountText(r.performance)}원" + (r.target?.let { " / 목표 ${amountText(it)}원" } ?: "") +
                    if (planned > 0) ", 예정 ${amountText(planned)}원" else "",
            )
        }
        Text(
            buildString {
                append("실적 ${amountText(r.performance)}")
                r.target?.let { append(" / ${amountText(it)}원") } ?: append("원")
                if (planned != 0L) append(" · 예정 ${signedWon(planned)}")
            },
            style = MaterialTheme.typography.bodySmall,
            color = muted,
        )
        Text(
            buildString {
                append("이번 달 혜택 ${benefitTotalsText(r)}")
                r.currentLimit?.let { append(" · 한도 ${amountText(r.limitUsed)} / ${amountText(it)}원") }
                val next = r.nextTier
                if (next != null && r.remainingToNextTier != null) {
                    append(" · +${amountText(r.remainingToNextTier)}원이면 다음 달 한도 ${amountText(next.benefitLimit)}원")
                } else {
                    r.achievedTier?.let { append(" · 다음 달 한도 ${amountText(it.benefitLimit)}원") }
                }
            },
            style = MaterialTheme.typography.bodySmall,
            color = muted,
        )
    }
}

@Composable
private fun FundsPanel(funds: Funds, ref: RefData, modifier: Modifier = Modifier) {
    val colors = chartColors()
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val upcoming = funds.cards
        .filter { c -> c.nextPayment?.let { it.confirmedAmount != 0L || it.plannedAmount != 0L } == true }
        .sortedBy { it.nextPayment?.paymentDate }
    Panel(
        modifier = modifier,
        title = "여유자금",
        sub = funds.balanceDate?.let { "오늘 기준 · 잔고 기준일 ${shortDate(it)}" } ?: "가계부 웹에서 은행 잔고를 입력하면 계산됩니다",
    ) {
        Text(
            "${amountText(funds.available)}원",
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.SemiBold,
            color = if (funds.available < 0) colors.badText else Color.Unspecified,
        )
        Breakdown("은행 잔고 합계", amountText(funds.accountsTotal))
        Breakdown("− 카드 결제 예정 (확정 이용분)", amountText(funds.cardDue))
        if (funds.unreflectedOutflow != 0L) Breakdown("− 잔고 반영 전 계좌 출금", amountText(funds.unreflectedOutflow))
        Breakdown("− 이번 달까지 예정 지출 (카드 ${amountText(funds.cardDuePlanned)} 포함)", amountText(funds.plannedOutflow + funds.cardDuePlanned))
        if (funds.plannedInflow != 0L) Breakdown("+ 예정 수입", amountText(funds.plannedInflow))
        Breakdown("= 예정 반영 후", "${amountText(funds.availableAfterPlanned)}원", bold = true, bad = funds.availableAfterPlanned < 0)
        if (upcoming.isNotEmpty()) {
            HorizontalDivider()
            Text("다가오는 카드 결제", style = MaterialTheme.typography.titleSmall)
            upcoming.forEach { c ->
                val p = c.nextPayment ?: return@forEach
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(ref.paymentMethod(c.paymentMethodId)?.name ?: "카드 #${c.paymentMethodId}", style = MaterialTheme.typography.bodyMedium)
                        Text(
                            "${shortDate(p.paymentDate)} 결제 · ${p.periodStart.drop(5).replace('-', '/')}~${p.periodEnd.drop(5).replace('-', '/')} 이용" +
                                (if (p.closed) "" else " (진행 중)") + if (!c.counted) " · 여유자금 제외 계좌" else "",
                            style = MaterialTheme.typography.bodySmall,
                            color = muted,
                        )
                    }
                    Column(horizontalAlignment = Alignment.End) {
                        Text("${amountText(p.confirmedAmount)}원", style = MaterialTheme.typography.bodyMedium)
                        if (p.plannedAmount != 0L) Text("+ 예정 ${amountText(p.plannedAmount)}", style = MaterialTheme.typography.bodySmall, color = muted)
                    }
                }
            }
        }
    }
}

@Composable
private fun Breakdown(label: String, value: String, bold: Boolean = false, bad: Boolean = false) {
    Row {
        Text(
            label,
            modifier = Modifier.weight(1f),
            style = MaterialTheme.typography.bodySmall,
            color = if (bold) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
            fontWeight = if (bold) FontWeight.SemiBold else null,
        )
        Spacer(Modifier.width(12.dp))
        Text(
            value,
            style = MaterialTheme.typography.bodySmall,
            fontWeight = if (bold) FontWeight.SemiBold else null,
            color = if (bad) chartColors().badText else MaterialTheme.colorScheme.onSurface,
            textAlign = TextAlign.End,
        )
    }
}

@Composable
private fun PlannedPanel(
    items: List<Tx>,
    ref: RefData,
    ledgerId: Int?,
    busyTx: Int?,
    onConfirm: (Tx) -> Unit,
    onSkip: (Tx) -> Unit,
    modifier: Modifier = Modifier,
) {
    var all by rememberSaveable { mutableStateOf(false) }
    val today = LocalDate.now()
    val overdue = items.filter { (parseDate(it.date) ?: today) < today }
    val upcoming = items.filter { (parseDate(it.date) ?: today) >= today }
    // 합계는 지출만 (수입과 장부 이체는 빼고)
    fun spending(list: List<Tx>) = list.filter { !it.isIncome && !it.isTransfer }.sumOf { it.amount }
    val limit = if (all) Int.MAX_VALUE else PLANNED_PREVIEW
    Panel(modifier = modifier, title = "예정 거래", sub = "반복 규칙(지출·월급·장부 이체)과 예정으로 입력한 거래 · 실제로 오가면 확정하세요") {
        if (items.isEmpty()) {
            EmptyText("확인할 예정 거래가 없습니다")
            return@Panel
        }
        if (overdue.isNotEmpty()) {
            Text(
                "날짜가 지났는데 확정되지 않은 ${overdue.size}건 (지출 ${amountText(spending(overdue))}원)",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
            overdue.take(limit).forEach { PlannedRow(it, ref, ledgerId, overdue = true, busy = busyTx != null, onConfirm, onSkip) }
        }
        if (upcoming.isNotEmpty()) {
            if (overdue.isNotEmpty()) HorizontalDivider()
            Text("앞으로 7일 · 지출 ${amountText(spending(upcoming))}원", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            upcoming.take(limit).forEach { PlannedRow(it, ref, ledgerId, overdue = false, busy = busyTx != null, onConfirm, onSkip) }
        }
        val hidden = (overdue.size - PLANNED_PREVIEW).coerceAtLeast(0) + (upcoming.size - PLANNED_PREVIEW).coerceAtLeast(0)
        if (hidden > 0 || all) {
            TextButton(onClick = { all = !all }, modifier = Modifier.align(Alignment.CenterHorizontally)) {
                Text(if (all) "접기" else "모두 보기 (${hidden}건 더)")
            }
        }
    }
}

private const val PLANNED_PREVIEW = 3

@Composable
private fun PlannedRow(tx: Tx, ref: RefData, ledgerId: Int?, overdue: Boolean, busy: Boolean, onConfirm: (Tx) -> Unit, onSkip: (Tx) -> Unit) {
    val colors = chartColors()
    val small = PaddingValues(horizontal = 10.dp, vertical = 0.dp)
    Column(Modifier.fillMaxWidth()) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "${shortDate(tx.date)}  ${tx.description}",
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            Text(
                txAmountText(tx, ledgerId),
                style = MaterialTheme.typography.bodyMedium,
                color = if (isInflow(tx, ledgerId)) colors.goodText else MaterialTheme.colorScheme.onSurface,
            )
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                txDetail(tx, ref, ledgerId, showLedger = ledgerId == null),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (overdue) TextButton(onClick = { onSkip(tx) }, enabled = !busy, contentPadding = small) { Text("건너뛰기") }
            OutlinedButton(onClick = { onConfirm(tx) }, enabled = !busy, contentPadding = small) { Text("확정") }
        }
    }
}

@Composable
private fun CategoryPanel(s: MonthlySummary, ref: RefData, ledgerId: Int?, modifier: Modifier = Modifier) {
    var all by rememberSaveable { mutableStateOf(false) }
    val colors = chartColors()
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    val rows = s.categories
        .filter { it.kind == "EXPENSE" && (it.confirmed != 0L || it.planned != 0L || it.budget != null) }
        .sortedByDescending { it.confirmed + it.planned }
    val max = rows.maxOfOrNull { maxOf((it.confirmed + it.planned).toDouble(), it.average, (it.budget ?: 0).toDouble()) }
        ?.coerceAtLeast(1.0) ?: 1.0
    Panel(
        modifier = modifier,
        title = "카테고리별 소비",
        sub = "막대 = 이번 달(밝은 부분은 예정), 세로선 = ${s.averageMonthsUsed}개월 평균",
    ) {
        if (rows.isEmpty()) {
            EmptyText("이번 달 소비가 없습니다")
            return@Panel
        }
        val shown = if (all) rows else rows.take(6)
        shown.forEach { c ->
            val total = c.confirmed + c.planned
            val over = c.budget != null && total > c.budget
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val ledger = if (ledgerId == null) ref.ledger(c.ledgerId)?.name?.let { "$it · " } ?: "" else ""
                    Text(ledger + c.name, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text("${amountText(c.confirmed)}원", style = MaterialTheme.typography.bodyMedium)
                }
                Meter(
                    value = c.confirmed,
                    planned = c.planned.coerceAtLeast(0),
                    max = max.roundToLong(),
                    ticks = listOf(c.average).filter { it > 0 },
                    goal = true,
                    track = false,
                    description = "${c.name} 이번 달 ${amountText(c.confirmed)}원, 평균 ${amountText(c.average)}원",
                )
                Text(
                    listOfNotNull(
                        c.planned.takeIf { it != 0L }?.let { "예정 ${signedWon(it)}" },
                        "평균 ${amountText(c.average)}",
                        c.budget?.let { "예산 ${amountText(it)}" },
                    ).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = muted,
                )
                if (over) {
                    Text("▲ 예산보다 ${amountText(total - c.budget)}원 많음 (예정 포함)", style = MaterialTheme.typography.bodySmall, color = colors.badText)
                }
            }
        }
        if (rows.size > 6) {
            TextButton(onClick = { all = !all }, modifier = Modifier.align(Alignment.CenterHorizontally)) {
                Text(if (all) "접기" else "모두 보기 (${rows.size})")
            }
        }
    }
}

@Composable
private fun EmptyText(text: String) {
    Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(vertical = 8.dp))
}
