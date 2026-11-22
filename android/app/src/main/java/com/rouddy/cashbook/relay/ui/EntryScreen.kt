package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.DateRange
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.OffsetMapping
import androidx.compose.ui.text.input.TransformedText
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.book.BookException
import com.rouddy.cashbook.relay.book.EntryForm
import com.rouddy.cashbook.relay.book.EntryKind
import com.rouddy.cashbook.relay.book.EntryRules
import com.rouddy.cashbook.relay.book.PAYMENT_KIND_LABEL
import com.rouddy.cashbook.relay.book.RefData
import com.rouddy.cashbook.relay.book.Suggestion
import com.rouddy.cashbook.relay.book.amountText
import com.rouddy.cashbook.relay.book.dayTitle
import com.rouddy.cashbook.relay.book.shortDate
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset
import kotlin.math.abs

class EntryViewModel : ViewModel() {
    var ref by mutableStateOf(Graph.book.ref.value)
        private set
    var form by mutableStateOf(EntryRules.newForm(ref, LocalDate.now()))
        private set
    var loadError by mutableStateOf<BookException?>(null)
        private set
    var loading by mutableStateOf(false)
        private set
    var saving by mutableStateOf(false)
        private set
    var suggestions by mutableStateOf<List<Suggestion>>(emptyList())
        private set
    var message by mutableStateOf<String?>(null)

    private var suggestJob: Job? = null

    fun loadRef() {
        if (loading) return
        loading = true
        viewModelScope.launch {
            try {
                val first = ref == null
                val r = Graph.book.loadRef()
                ref = r
                form = if (first) EntryRules.newForm(r, form.date) else keepValid(form, r)
                loadError = null
            } catch (e: BookException) {
                loadError = e
                // 다른 서버·토큰으로 바뀌었으면 이전 목록으로 입력하지 않게 한다
                if (e.fixInSettings) ref = null
            } finally {
                loading = false
            }
        }
    }

    /** 목록이 바뀌어(보관·삭제) 없어진 값은 비운다 */
    private fun keepValid(f: EntryForm, r: RefData): EntryForm {
        val ledger = r.ledger(f.ledgerId)?.id ?: r.defaultLedger?.id
        var next = EntryRules.withLedger(f, ledger ?: return f, r)
        if (r.category(next.categoryId)?.archived != false) next = next.copy(categoryId = null)
        if (r.paymentMethod(next.paymentMethodId)?.archived != false) next = EntryRules.withPaymentMethod(next, null, r)
        return next
    }

    fun update(transform: (EntryForm, RefData) -> EntryForm) {
        val r = ref ?: return
        form = transform(form, r)
    }

    fun setDescription(text: String) {
        form = form.copy(description = text)
        suggestJob?.cancel()
        val q = text.trim()
        if (q.isEmpty() || form.kind == EntryKind.TRANSFER) {
            suggestions = emptyList()
            return
        }
        suggestJob = viewModelScope.launch {
            delay(250)
            suggestions = try {
                Graph.book.suggestions(q, LocalDate.now()).filter { it.description != q }
            } catch (_: BookException) {
                emptyList()
            }
        }
    }

    fun pick(s: Suggestion) {
        suggestJob?.cancel()
        suggestions = emptyList()
        update { f, r -> EntryRules.applySuggestion(f, s, r) }
    }

    fun dismissSuggestions() {
        suggestJob?.cancel()
        suggestions = emptyList()
    }

    fun reset() {
        form = EntryRules.newForm(ref, LocalDate.now())
        dismissSuggestions()
    }

    fun save() {
        val r = ref ?: return
        if (saving) return
        val input = EntryRules.toInput(form, r).getOrElse {
            message = it.message
            return
        }
        saving = true
        val saved = form
        viewModelScope.launch {
            try {
                val tx = Graph.book.create(input)
                val what = when (saved.kind) {
                    EntryKind.TRANSFER -> "${r.ledger(saved.ledgerId)?.name} → ${r.ledger(saved.toLedgerId)?.name} 이체"
                    else -> tx.description
                }
                message = "$what ${amountText(abs(tx.amount))}원 ${if (tx.status == "PLANNED") "예정으로 " else ""}저장했습니다"
                form = EntryRules.afterSave(saved)
                dismissSuggestions()
            } catch (e: BookException) {
                message = e.message
            } finally {
                saving = false
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EntryScreen(onOpenSettings: () -> Unit) {
    val vm: EntryViewModel = viewModel()
    val snackbar = remember { SnackbarHostState() }
    LifecycleResumeEffect(Unit) {
        vm.loadRef()
        onPauseOrDispose { }
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
                title = { Text("수동 입력") },
                actions = {
                    TextButton(onClick = vm::reset, enabled = vm.ref != null) { Text("초기화") }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        val ref = vm.ref
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            vm.loadError?.let { BookErrorCard(it, onRetry = vm::loadRef, onOpenSettings = onOpenSettings) }
            if (ref == null) {
                if (vm.loading) CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally).padding(24.dp))
                return@Column
            }
            EntryFields(vm, ref)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
private fun EntryFields(vm: EntryViewModel, ref: RefData) {
    val f = vm.form
    val transfer = f.kind == EntryKind.TRANSFER
    val expense = f.kind == EntryKind.EXPENSE

    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        EntryKind.entries.forEachIndexed { i, kind ->
            SegmentedButton(
                selected = f.kind == kind,
                onClick = {
                    vm.update { form, r -> EntryRules.withKind(form, kind, r) }
                    vm.dismissSuggestions()
                },
                shape = SegmentedButtonDefaults.itemShape(i, EntryKind.entries.size),
            ) { Text(kind.label) }
        }
    }

    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
            value = f.amount.filter(Char::isDigit),
            onValueChange = { text -> vm.update { form, _ -> form.copy(amount = text.filter(Char::isDigit).take(12)) } },
            label = { Text(if (transfer) "옮길 금액" else "금액") },
            suffix = { Text("원") },
            prefix = if (expense && f.refund) ({ Text("−") }) else null,
            singleLine = true,
            textStyle = MaterialTheme.typography.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
            visualTransformation = ThousandsTransformation,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Next),
            modifier = Modifier.weight(1f),
        )
        if (expense) {
            FilterChip(
                selected = f.refund,
                onClick = { vm.update { form, _ -> form.copy(refund = !form.refund) } },
                label = { Text("환불·취소") },
            )
        }
    }

    DescriptionField(vm, transfer)

    DateRow(f.date) { date -> vm.update { form, _ -> form.copy(date = date) } }

    LabeledChips(if (transfer) "보내는 장부" else "장부") {
        ref.ledgers.forEach { l ->
            FilterChip(
                selected = f.ledgerId == l.id,
                onClick = { vm.update { form, r -> EntryRules.withLedger(form, l.id, r) } },
                label = { Text(l.name) },
            )
        }
    }

    if (transfer) {
        LabeledChips("받는 장부") {
            ref.ledgers.filter { it.id != f.ledgerId }.forEach { l ->
                FilterChip(
                    selected = f.toLedgerId == l.id,
                    onClick = { vm.update { form, _ -> form.copy(toLedgerId = l.id) } },
                    label = { Text(l.name) },
                )
            }
        }
        if (ref.ledgers.size < 2) Hint("장부가 하나뿐입니다. 가계부 웹에서 장부를 더 만들면 장부끼리 돈을 옮길 수 있습니다.")
        Hint("장부 이체는 소비·수입이 아니라 보낸 장부의 월잔액에서 빠지고 받은 장부의 월잔액에 더해집니다.")
    } else {
        val categories = ref.categoriesFor(f.ledgerId, income = f.kind == EntryKind.INCOME)
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            FieldLabel("항목")
            if (categories.isEmpty()) {
                Hint("이 장부에 ${if (expense) "지출" else "수입"} 항목이 없습니다. 미분류로 저장합니다.")
            } else {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    categories.forEach { c ->
                        FilterChip(
                            selected = f.categoryId == c.id,
                            onClick = { vm.update { form, _ -> form.copy(categoryId = if (form.categoryId == c.id) null else c.id) } },
                            label = { Text(c.name) },
                        )
                    }
                }
            }
        }

        val methods = ref.activePaymentMethods
        ChoiceField(
            label = if (expense) "결제수단" else "결제수단 (받은 곳, 선택)",
            value = f.paymentMethodId?.toString().orEmpty(),
            choices = methods.map { Choice(value = it.id.toString(), label = it.name, group = PAYMENT_KIND_LABEL[it.kind] ?: it.kind) },
            emptyLabel = "선택 안 함",
            onValueChange = {},
            onPick = { choice -> vm.update { form, r -> EntryRules.withPaymentMethod(form, choice?.value?.toIntOrNull(), r) } },
            modifier = Modifier.fillMaxWidth(),
        )

        val pm = ref.paymentMethod(f.paymentMethodId)
        if (expense && pm?.isCreditCard == true) {
            ChoiceField(
                label = "할부",
                value = f.installmentMonths.toString(),
                choices = EntryRules.INSTALLMENT_CHOICES.map { Choice(it.toString(), if (it == 1) "일시불" else "${it}개월") },
                emptyLabel = "일시불",
                onValueChange = {},
                onPick = { choice -> vm.update { form, _ -> form.copy(installmentMonths = choice?.value?.toIntOrNull() ?: 1) } },
                allowEmpty = false,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (expense && pm != null && pm.isCard) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                if (pm.benefits.isNotEmpty()) {
                    FieldLabel("${pm.name} 혜택")
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        pm.benefits.forEach { b ->
                            val on = b.id in f.benefitIds
                            FilterChip(
                                selected = on,
                                onClick = { vm.update { form, _ -> form.copy(benefitIds = EntryRules.toggleBenefit(form.benefitIds, b, !on, pm.benefits)) } },
                                label = { Text(b.name) },
                            )
                        }
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(
                        checked = f.excludeFromPerformance,
                        onCheckedChange = { on -> vm.update { form, _ -> form.copy(excludeFromPerformance = on) } },
                    )
                    Text("전월실적에서 제외", style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
    }

    MemberField(f.member, ref.members) { m -> vm.update { form, _ -> form.copy(member = m) } }

    OutlinedTextField(
        value = f.memo,
        onValueChange = { m -> vm.update { form, _ -> form.copy(memo = m) } },
        label = { Text("메모") },
        modifier = Modifier.fillMaxWidth(),
    )

    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        FieldLabel("상태")
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
            listOf(false to "확정 (이미 오감)", true to "예정 (오갈 예정)").forEachIndexed { i, (planned, label) ->
                SegmentedButton(
                    selected = f.planned == planned,
                    onClick = { vm.update { form, _ -> form.copy(planned = planned) } },
                    shape = SegmentedButtonDefaults.itemShape(i, 2),
                ) { Text(label) }
            }
        }
    }

    Button(onClick = vm::save, enabled = !vm.saving, modifier = Modifier.fillMaxWidth().height(52.dp)) {
        if (vm.saving) CircularProgressIndicator(Modifier.height(20.dp)) else Text("저장")
    }
    Hint("저장하면 가계부의 장부·카드별 내역·대시보드에 바로 반영됩니다.")
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DescriptionField(vm: EntryViewModel, transfer: Boolean) {
    val suggestions = vm.suggestions
    ExposedDropdownMenuBox(expanded = suggestions.isNotEmpty(), onExpandedChange = { if (!it) vm.dismissSuggestions() }) {
        OutlinedTextField(
            value = vm.form.description,
            onValueChange = vm::setDescription,
            label = { Text("내용") },
            placeholder = { Text(if (transfer) EntryRules.TRANSFER_DEFAULT_DESCRIPTION else "가맹점·내용") },
            supportingText = { Text(if (transfer) "비우면 '${EntryRules.TRANSFER_DEFAULT_DESCRIPTION}'" else "과거에 쓴 내용을 고르면 장부·항목·결제수단을 채웁니다") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryEditable),
        )
        ExposedDropdownMenu(expanded = suggestions.isNotEmpty(), onDismissRequest = vm::dismissSuggestions) {
            val ref = vm.ref
            suggestions.forEach { s ->
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(s.description, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                listOfNotNull(
                                    ref?.category(s.categoryId)?.name,
                                    ref?.paymentMethod(s.paymentMethodId)?.name,
                                    s.lastAmount.takeIf { it > 0 }?.let { "최근 ${amountText(it)}원" },
                                    s.lastDate?.let(::shortDate),
                                ).joinToString(" · "),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    },
                    onClick = { vm.pick(s) },
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DateRow(date: LocalDate, onChange: (LocalDate) -> Unit) {
    var picking by remember { mutableStateOf(false) }
    val today = LocalDate.now()
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        FieldLabel("날짜")
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { picking = true }) {
                Icon(Icons.Default.DateRange, contentDescription = null)
                Text("  ${dayTitle(date)}" + if (date.year != today.year) " ${date.year}" else "")
            }
            FilterChip(selected = date == today, onClick = { onChange(today) }, label = { Text("오늘") })
            FilterChip(selected = date == today.minusDays(1), onClick = { onChange(today.minusDays(1)) }, label = { Text("어제") })
        }
    }
    if (picking) {
        // DatePicker는 UTC 자정 기준 밀리초를 쓴다
        val state = rememberDatePickerState(initialSelectedDateMillis = date.atStartOfDay().toInstant(ZoneOffset.UTC).toEpochMilli())
        DatePickerDialog(
            onDismissRequest = { picking = false },
            confirmButton = {
                TextButton(onClick = {
                    state.selectedDateMillis?.let { onChange(Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate()) }
                    picking = false
                }) { Text("확인") }
            },
            dismissButton = { TextButton(onClick = { picking = false }) { Text("취소") } },
        ) { DatePicker(state) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MemberField(value: String, members: List<String>, onChange: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    val matches = members.filter { it != value && (value.isBlank() || it.contains(value.trim(), ignoreCase = true)) }
    ExposedDropdownMenuBox(expanded = expanded && matches.isNotEmpty(), onExpandedChange = { expanded = it }) {
        OutlinedTextField(
            value = value,
            onValueChange = {
                onChange(it)
                expanded = true
            },
            label = { Text("구성원 (선택)") },
            placeholder = { Text("실제로 쓴 사람") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryEditable),
        )
        ExposedDropdownMenu(expanded = expanded && matches.isNotEmpty(), onDismissRequest = { expanded = false }) {
            matches.forEach { m ->
                DropdownMenuItem(text = { Text(m) }, onClick = {
                    onChange(m)
                    expanded = false
                })
            }
        }
    }
}

@Composable
private fun LabeledChips(label: String, chips: @Composable () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        FieldLabel(label)
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) { chips() }
    }
}

@Composable
private fun FieldLabel(text: String) {
    Text(text, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

@Composable
private fun Hint(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

/** 숫자만 든 값에 세 자리마다 쉼표를 보이게 한다 (커서 위치도 맞춤) */
object ThousandsTransformation : VisualTransformation {
    override fun filter(text: AnnotatedString): TransformedText {
        val digits = text.text
        val out = StringBuilder()
        // 원래 위치 i → 바뀐 위치
        val map = IntArray(digits.length + 1)
        digits.forEachIndexed { i, c ->
            if (i > 0 && (digits.length - i) % 3 == 0) out.append(',')
            map[i] = out.length
            out.append(c)
        }
        map[digits.length] = out.length
        val transformed = out.toString()
        return TransformedText(
            AnnotatedString(transformed),
            object : OffsetMapping {
                override fun originalToTransformed(offset: Int): Int = map[offset.coerceIn(0, digits.length)]

                override fun transformedToOriginal(offset: Int): Int {
                    val o = offset.coerceIn(0, transformed.length)
                    return map.indexOfFirst { it >= o }.let { if (it < 0) digits.length else it }
                }
            },
        )
    }
}

