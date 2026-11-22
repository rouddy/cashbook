package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.ExperimentalFoundationApi
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.book.BookException
import com.rouddy.cashbook.relay.book.RefData
import com.rouddy.cashbook.relay.book.SOURCE_LABEL
import com.rouddy.cashbook.relay.book.Tx
import com.rouddy.cashbook.relay.book.amountText
import com.rouddy.cashbook.relay.book.dayTitle
import com.rouddy.cashbook.relay.book.isInflow
import com.rouddy.cashbook.relay.book.parseDate
import com.rouddy.cashbook.relay.book.transferLabel
import com.rouddy.cashbook.relay.book.txAmountText
import com.rouddy.cashbook.relay.book.txDetail
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch

/**
 * 최근 내역: 확정 거래를 날짜 최신순으로 [PAGE]건씩 이어서 읽는다.
 * "자동 입력만"(source = API)은 SharedPreferences에 마지막 상태를 둔다.
 */
class RecentViewModel : ViewModel() {
    var autoOnly by mutableStateOf(Graph.prefs.recentAutoOnly)
        private set
    val items = mutableStateListOf<Tx>()
    var total by mutableLongStateOf(0)
        private set
    var ref by mutableStateOf(Graph.book.ref.value)
        private set

    /** 첫 쪽(또는 새로 고침)을 읽는 중 */
    var loading by mutableStateOf(false)
        private set
    var refreshing by mutableStateOf(false)
        private set

    /** 다음 쪽을 읽는 중 */
    var loadingMore by mutableStateOf(false)
        private set
    var error by mutableStateOf<BookException?>(null)
        private set

    /** 다음 쪽 읽기 실패 (목록은 그대로 두고 아래에 다시 시도 버튼) */
    var moreError by mutableStateOf<BookException?>(null)
        private set
    var loaded by mutableStateOf(false)
        private set

    private var generation = 0
    private var visible = false

    val endReached: Boolean get() = loaded && items.size.toLong() >= total

    init {
        viewModelScope.launch { Graph.book.changes.drop(1).collect { if (visible) reload(keepCount = true) } }
    }

    fun onVisible(isVisible: Boolean) {
        visible = isVisible
        if (isVisible) reload(keepCount = true, refreshRef = ref == null)
    }

    fun changeAutoOnly(on: Boolean) {
        if (on == autoOnly) return
        autoOnly = on
        Graph.prefs.recentAutoOnly = on
        items.clear()
        loaded = false
        reload()
    }

    fun refresh() = reload(user = true, refreshRef = true)

    /**
     * 첫 쪽부터 다시 읽는다. [keepCount]면 지금까지 불러온 만큼(최대 [MAX_RELOAD]건) 한 번에 다시 읽어
     * 새 거래가 들어와도 보던 자리가 줄어들지 않게 한다.
     */
    fun reload(user: Boolean = false, keepCount: Boolean = false, refreshRef: Boolean = false) {
        val gen = ++generation
        val limit = if (keepCount) items.size.coerceIn(PAGE, MAX_RELOAD) else PAGE
        loading = true
        loadingMore = false
        moreError = null
        if (user) refreshing = true
        viewModelScope.launch {
            try {
                if (refreshRef || ref == null) ref = Graph.book.loadRef()
                val page = Graph.book.transactions(autoOnly, offset = 0, limit = limit)
                if (gen != generation) return@launch
                items.clear()
                items.addAll(page.items)
                total = page.totalCount
                loaded = true
                error = null
            } catch (e: BookException) {
                if (gen == generation) {
                    error = e
                    // 다른 서버·토큰으로 바뀌었으면 이전 목록을 보이지 않는다
                    if (e.fixInSettings) {
                        items.clear()
                        loaded = false
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

    fun loadMore() {
        if (loading || loadingMore || endReached || !loaded) return
        val gen = generation
        loadingMore = true
        moreError = null
        viewModelScope.launch {
            try {
                val page = Graph.book.transactions(autoOnly, offset = items.size, limit = PAGE)
                if (gen != generation) return@launch
                // 그사이 새 거래가 들어와 밀린 행은 이미 있으니 건너뛴다
                val have = items.mapTo(HashSet()) { it.id }
                items.addAll(page.items.filter { it.id !in have })
                total = if (page.items.isEmpty()) items.size.toLong() else page.totalCount
            } catch (e: BookException) {
                if (gen == generation) moreError = e
            } finally {
                if (gen == generation) loadingMore = false
            }
        }
    }

    companion object {
        const val PAGE = 30
        const val MAX_RELOAD = 300
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecentScreen(onOpenSettings: () -> Unit) {
    val vm: RecentViewModel = viewModel()
    var selected by remember { mutableStateOf<Tx?>(null) }
    LifecycleResumeEffect(Unit) {
        vm.onVisible(true)
        onPauseOrDispose { vm.onVisible(false) }
    }
    Scaffold(topBar = { TopAppBar(title = { Text("최근 내역") }) }) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            Row(
                Modifier.fillMaxWidth().clickable { vm.changeAutoOnly(!vm.autoOnly) }.padding(start = 4.dp, end = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Checkbox(checked = vm.autoOnly, onCheckedChange = vm::changeAutoOnly)
                Text("자동 입력만 보기", style = MaterialTheme.typography.bodyLarge)
                Spacer(Modifier.weight(1f))
                if (vm.loaded) {
                    Text("${amountText(vm.total)}건", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            HorizontalDivider()
            PullToRefreshBox(isRefreshing = vm.refreshing, onRefresh = vm::refresh, modifier = Modifier.fillMaxSize()) {
                RecentList(vm, onOpenSettings, onSelect = { selected = it })
            }
        }
    }
    selected?.let { tx -> TxSheet(tx, vm.ref, onDismiss = { selected = null }) }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun RecentList(vm: RecentViewModel, onOpenSettings: () -> Unit, onSelect: (Tx) -> Unit) {
    val listState = rememberLazyListState()
    // 끝에서 다섯 줄 안쪽까지 내려오면 다음 쪽을 읽는다
    LaunchedEffect(listState, vm) {
        snapshotFlow {
            val info = listState.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull()?.index ?: -1
            info.totalItemsCount > 0 && last >= info.totalItemsCount - 5
        }
            .distinctUntilChanged()
            .filter { it }
            .collect { vm.loadMore() }
    }
    // 다음 쪽을 다 읽었는데도 화면이 차지 않으면 이어서 읽는다
    LaunchedEffect(vm.items.size, vm.loadingMore) {
        val info = listState.layoutInfo
        val last = info.visibleItemsInfo.lastOrNull()?.index ?: -1
        if (!vm.loadingMore && info.totalItemsCount > 0 && last >= info.totalItemsCount - 5) vm.loadMore()
    }
    val ref = vm.ref
    val groups = vm.items.groupBy { it.date }
    // 거래 행만 키를 준다: 처음 읽을 때 맨 아래 줄(footer)에 키가 있으면 목록이 그 줄을 따라 첫 쪽 끝으로 밀린다
    LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(bottom = 24.dp)) {
        vm.error?.let { e ->
            item {
                BookErrorCard(e, onRetry = vm::refresh, onOpenSettings = onOpenSettings, modifier = Modifier.padding(16.dp))
            }
        }
        if (vm.loaded && vm.items.isEmpty()) {
            item {
                Text(
                    if (vm.autoOnly) "자동 입력된 거래가 없습니다. 설정 → 규칙에서 카드 문자·앱 알림 규칙을 만드세요." else "거래가 없습니다.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(24.dp),
                )
            }
        }
        groups.forEach { (date, txs) ->
            stickyHeader(key = "d-$date") { DateHeader(date) }
            items(txs, key = { it.id }) { tx ->
                TxRow(tx, ref, onClick = { onSelect(tx) })
                HorizontalDivider()
            }
        }
        item {
            Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                val more = vm.moreError
                when {
                    vm.loadingMore || (vm.loading && vm.items.isEmpty() && !vm.refreshing) -> CircularProgressIndicator()
                    more != null -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(more.message.orEmpty(), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                        OutlinedButton(onClick = vm::loadMore) { Text("다시 시도") }
                    }
                    vm.loaded && vm.items.isNotEmpty() && vm.endReached ->
                        Text("모두 불러왔습니다", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

@Composable
private fun DateHeader(date: String) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Text(
            parseDate(date)?.let(::dayTitle) ?: date,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp),
        )
    }
}

@Composable
private fun TxRow(tx: Tx, ref: RefData?, onClick: () -> Unit) {
    val colors = chartColors()
    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        headlineContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(tx.description, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                if (tx.isAuto) {
                    Spacer(Modifier.width(6.dp))
                    Badge("자동")
                }
                if (tx.splitParentId != null) {
                    Spacer(Modifier.width(6.dp))
                    Badge("분할")
                }
            }
        },
        supportingContent = { Text(txDetail(tx, ref, null, showLedger = true), maxLines = 1, overflow = TextOverflow.Ellipsis) },
        trailingContent = {
            Text(
                txAmountText(tx, null),
                style = MaterialTheme.typography.bodyLarge,
                color = if (isInflow(tx, null)) colors.goodText else MaterialTheme.colorScheme.onSurface,
            )
        },
    )
}

@Composable
private fun Badge(text: String) {
    Surface(color = MaterialTheme.colorScheme.secondaryContainer, shape = MaterialTheme.shapes.extraSmall) {
        Text(
            text,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSecondaryContainer,
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 1.dp),
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TxSheet(tx: Tx, ref: RefData?, onDismiss: () -> Unit) {
    ModalBottomSheet(onDismissRequest = onDismiss) {
        SelectionContainer {
            Column(
                Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(start = 24.dp, end = 24.dp, bottom = 32.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Text(tx.description, style = MaterialTheme.typography.titleLarge)
                Text(txAmountText(tx, null), style = MaterialTheme.typography.headlineSmall)
                SheetField("날짜", parseDate(tx.date)?.let { "${it.year}년 ${dayTitle(it)}" } ?: tx.date)
                SheetField("장부", ref?.ledger(tx.ledgerId)?.name ?: "#${tx.ledgerId}")
                if (tx.isTransfer) {
                    SheetField("장부 이체", transferLabel(tx, ref, null).orEmpty())
                } else {
                    SheetField("항목", ref?.category(tx.categoryId)?.name ?: "미분류")
                    ref?.paymentMethod(tx.paymentMethodId)?.let { pm ->
                        SheetField("결제수단", pm.name + if (tx.installmentMonths > 1) " · ${tx.installmentMonths}개월 할부" else "")
                        val benefits = pm.benefits.filter { it.id in tx.benefitIds }.map { it.name }
                        if (benefits.isNotEmpty()) SheetField("카드 혜택", benefits.joinToString(", "))
                    }
                }
                tx.member?.let { SheetField("구성원", it) }
                tx.memo?.let { SheetField("메모", it) }
                if (tx.splitParentId != null) SheetField("분할", "다른 장부와 나눈 결제의 일부입니다 (가계부 웹에서 확인)")
                SheetField("입력 경로", SOURCE_LABEL[tx.source] ?: tx.source)
                tx.rawText?.takeIf { it.isNotBlank() }?.let { SheetField("원문", it, mono = true) }
            }
        }
    }
}

@Composable
private fun SheetField(label: String, value: String, mono: Boolean = false) {
    Column {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Surface(color = if (mono) MaterialTheme.colorScheme.surfaceVariant else Color.Transparent, shape = MaterialTheme.shapes.small) {
            Text(
                value,
                style = if (mono) MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace) else MaterialTheme.typography.bodyLarge,
                modifier = if (mono) Modifier.padding(8.dp) else Modifier,
            )
        }
    }
}
