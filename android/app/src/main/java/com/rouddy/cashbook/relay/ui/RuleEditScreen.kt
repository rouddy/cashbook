package com.rouddy.cashbook.relay.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.CardMapping
import com.rouddy.cashbook.relay.data.ParseMode
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.ServerOptions
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.ingest.CardSource
import com.rouddy.cashbook.relay.ingest.CashbookApi
import com.rouddy.cashbook.relay.ingest.RuleMatcher
import com.rouddy.cashbook.relay.ingest.TestResult
import com.rouddy.cashbook.relay.parse.Field
import com.rouddy.cashbook.relay.parse.PhoneNumber
import com.rouddy.cashbook.relay.parse.WILDCARD
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import java.time.LocalDate

class RuleEditViewModel(ruleId: String?, source: SourceType) : ViewModel() {
    /** 저장된 값 (바뀐 게 있는지 비교용) */
    var original by mutableStateOf(Rule(source = source))
        private set
    var rule by mutableStateOf(original)
        private set
    var format by mutableStateOf(TextFieldValue(""))
        private set
    val isNew = ruleId == null
    var loaded by mutableStateOf(ruleId == null)
        private set

    /** 가계부에서 받은 장부·카테고리·결제수단 목록. 없으면 직접 입력한다 */
    var options by mutableStateOf<ServerOptions?>(null)
        private set
    var optionsLoading by mutableStateOf(false)
        private set
    var optionsError by mutableStateOf<String?>(null)
        private set

    init {
        viewModelScope.launch {
            // 같은 서버에서 받아 둔 목록을 먼저 쓰고 새로 받는다
            val config = Graph.store.config()
            val url = config.settings.serverUrl.trim().trimEnd('/')
            options = config.serverOptions?.takeIf { url.isNotEmpty() && it.serverUrl == url }
            fetchOptions()
        }
        if (ruleId != null) {
            viewModelScope.launch {
                Graph.store.config().rules.firstOrNull { it.id == ruleId }?.let {
                    original = it
                    rule = it
                    format = TextFieldValue(it.format)
                }
                loaded = true
            }
        }
    }

    val dirty: Boolean get() = rule != original

    fun reloadOptions() {
        if (!optionsLoading) viewModelScope.launch { fetchOptions() }
    }

    private suspend fun fetchOptions() {
        val settings = Graph.store.config().settings
        if (settings.serverUrl.isBlank()) {
            optionsError = "서버 설정에 주소와 토큰을 넣으면 가계부의 목록에서 고를 수 있습니다"
            return
        }
        optionsLoading = true
        val result = CashbookApi(settings).options()
        result.getOrNull()?.let {
            options = it
            optionsError = null
            Graph.store.saveServerOptions(it)
        }
        result.exceptionOrNull()?.let { optionsError = it.message ?: "목록을 불러오지 못했습니다" }
        optionsLoading = false
    }

    fun addMapping(match: String = "") {
        rule = rule.copy(cardMappings = rule.cardMappings + CardMapping(match = match))
    }

    fun updateMapping(index: Int, transform: (CardMapping) -> CardMapping) {
        rule = rule.copy(cardMappings = rule.cardMappings.mapIndexed { i, m -> if (i == index) transform(m) else m })
    }

    fun removeMapping(index: Int) {
        rule = rule.copy(cardMappings = rule.cardMappings.filterIndexed { i, _ -> i != index })
    }

    fun update(transform: (Rule) -> Rule) {
        rule = transform(rule)
    }

    fun changeFormat(value: TextFieldValue) {
        format = value
        rule = rule.copy(format = value.text)
    }

    /** 커서 위치에 항목을 넣는다 (선택한 글자가 있으면 바꾼다) */
    fun insert(token: String) {
        val sel = format.selection
        val text = format.text.replaceRange(sel.min, sel.max, token)
        changeFormat(TextFieldValue(text, TextRange(sel.min + token.length)))
    }

    fun addSender(raw: String): Boolean {
        val number = raw.trim()
        if (number.isEmpty() || rule.senders.any { PhoneNumber.same(it, number) }) return false
        rule = rule.copy(senders = rule.senders + number)
        return true
    }

    /** 저장. 문제가 있으면 메시지를 돌려준다. */
    suspend fun save(): String? {
        val r = rule
        if (r.source == SourceType.NOTIFICATION && r.packageName.isBlank()) return "알림을 받을 앱을 선택하세요"
        RuleMatcher.formatError(r)?.let { return "형식 오류: $it" }
        val mappings = r.cardMappings
            .map { it.copy(match = it.match.trim(), card = it.card.trim()) }
            .filterNot { it.match.isEmpty() && it.card.isEmpty() }
        mappings.firstOrNull { it.match.isEmpty() }?.let { return "카드별 결제수단: '${it.card}'에 맞출 카드 값을 입력하세요" }
        mappings.firstOrNull { it.card.isEmpty() }?.let { return "카드별 결제수단: '${it.match}'의 결제수단을 고르세요" }
        // 이름을 비우면 번호/앱 이름으로 채운다 (기록·실패 알림에 규칙 이름으로 보인다)
        val saved = r.copy(
            name = r.name.trim().ifEmpty { r.displayName() },
            keywords = r.keywords.trim(),
            titleContains = r.titleContains.trim(),
            card = r.card.trim(),
            ledger = r.ledger.trim(),
            category = r.category.trim(),
            cardMappings = mappings,
        )
        Graph.store.saveRule(saved)
        original = saved
        rule = saved
        return null
    }

    suspend fun delete() {
        Graph.store.deleteRule(rule.id)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RuleEditScreen(ruleId: String?, source: SourceType, onDone: () -> Unit) {
    val vm: RuleEditViewModel = viewModel(key = ruleId ?: "new-$source") { RuleEditViewModel(ruleId, source) }
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
    var confirmLeave by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    var picker by remember { mutableStateOf<Picker?>(null) }
    val rule = vm.rule
    val test = remember(rule) { RuleMatcher.test(rule, rule.sample, LocalDate.now()) }

    BackHandler(enabled = vm.dirty) { confirmLeave = true }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text((if (vm.isNew) "새 " else "") + if (source == SourceType.SMS) "문자 규칙" else "앱 알림 규칙") },
                navigationIcon = {
                    IconButton(onClick = { if (vm.dirty) confirmLeave = true else onDone() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "뒤로")
                    }
                },
                actions = {
                    if (!vm.isNew) {
                        IconButton(onClick = { confirmDelete = true }) { Icon(Icons.Default.Delete, contentDescription = "삭제") }
                    }
                    TextButton(onClick = {
                        scope.launch {
                            val error = vm.save()
                            if (error == null) onDone() else snackbar.showSnackbar(error)
                        }
                    }) { Text("저장") }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        if (!vm.loaded) return@Scaffold
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            OutlinedTextField(
                value = rule.name,
                onValueChange = { v -> vm.update { it.copy(name = v) } },
                label = { Text("이름") },
                placeholder = { Text(rule.copy(name = "").displayName()) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("사용", Modifier.weight(1f))
                Switch(checked = rule.enabled, onCheckedChange = { on -> vm.update { it.copy(enabled = on) } })
            }

            HorizontalDivider()
            when (source) {
                SourceType.SMS -> SmsTarget(vm, onPickSender = { picker = Picker.Sender })
                SourceType.NOTIFICATION -> NotificationTarget(vm, onPickApp = { picker = Picker.App })
            }
            OutlinedTextField(
                value = rule.keywords,
                onValueChange = { v -> vm.update { it.copy(keywords = v) } },
                label = { Text("본문에 포함할 단어 (선택)") },
                supportingText = { Text("쉼표로 여러 개. 하나라도 있어야 처리합니다. 예: 승인") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            HorizontalDivider()
            FormatSection(vm)

            HorizontalDivider()
            ServerValuesSection(vm, test)

            HorizontalDivider()
            TestSection(vm, test, onPickSample = { picker = Picker.Sample })
            Spacer(Modifier.height(32.dp))
        }
    }

    when (picker) {
        Picker.App -> AppPickerDialog(
            onDismiss = { picker = null },
            onPick = { pkg, label, sample ->
                vm.update { it.copy(packageName = pkg, appLabel = label, sample = sample ?: it.sample) }
                picker = null
            },
        )
        Picker.Sender -> SenderPickerDialog(
            onDismiss = { picker = null },
            onPick = { address, sample ->
                vm.addSender(address)
                if (vm.rule.sample.isBlank()) vm.update { it.copy(sample = sample) }
                picker = null
            },
        )
        Picker.Sample -> SamplePickerDialog(
            rule = rule,
            onDismiss = { picker = null },
            onPick = { text ->
                vm.update { it.copy(sample = text) }
                picker = null
            },
        )
        null -> Unit
    }

    if (confirmLeave) {
        AlertDialog(
            onDismissRequest = { confirmLeave = false },
            title = { Text("저장하지 않고 나갈까요?") },
            text = { Text("바꾼 내용이 사라집니다.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmLeave = false
                    onDone()
                }) { Text("나가기") }
            },
            dismissButton = { TextButton(onClick = { confirmLeave = false }) { Text("계속 편집") } },
        )
    }
    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text("규칙 삭제") },
            text = { Text("'${rule.displayName()}' 규칙을 지울까요?") },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    scope.launch {
                        vm.delete()
                        onDone()
                    }
                }) { Text("삭제") }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text("취소") } },
        )
    }
}

private enum class Picker { App, Sender, Sample }

@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
@Composable
private fun SmsTarget(vm: RuleEditViewModel, onPickSender: () -> Unit) {
    val rule = vm.rule
    var input by remember { mutableStateOf("") }
    SectionTitle("보내는 번호")
    if (rule.senders.isNotEmpty()) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            rule.senders.forEach { number ->
                InputChip(
                    selected = false,
                    onClick = {},
                    label = { Text(PhoneNumber.format(number)) },
                    trailingIcon = {
                        IconButton(onClick = { vm.update { it.copy(senders = it.senders - number) } }, modifier = Modifier.size(24.dp)) {
                            Icon(Icons.Default.Close, contentDescription = "$number 빼기", modifier = Modifier.size(18.dp))
                        }
                    },
                )
            }
        }
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        OutlinedTextField(
            value = input,
            onValueChange = { input = it },
            label = { Text("번호 추가") },
            placeholder = { Text("예: 1544-7200") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Phone),
            modifier = Modifier.weight(1f),
        )
        TextButton(enabled = input.isNotBlank(), onClick = { if (vm.addSender(input)) input = "" }) { Text("추가") }
    }
    OutlinedButton(onClick = onPickSender, modifier = Modifier.fillMaxWidth()) { Text("받은 문자에서 번호 고르기") }
    Text(
        if (rule.senders.isEmpty()) {
            "번호를 비우면 모든 문자에 적용됩니다 (아래 형식이 맞는 문자만 보냅니다)."
        } else {
            "이 번호에서 온 문자(장문 LMS 포함)만 처리합니다. RCS(채팅+)로 오는 메시지는 문자 앱 알림 규칙으로 받으세요."
        },
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}

@Composable
private fun NotificationTarget(vm: RuleEditViewModel, onPickApp: () -> Unit) {
    val rule = vm.rule
    SectionTitle("앱")
    if (rule.packageName.isBlank()) {
        OutlinedButton(onClick = onPickApp, modifier = Modifier.fillMaxWidth()) { Text("앱 선택") }
    } else {
        OutlinedCard(Modifier.fillMaxWidth()) {
            ListItem(
                leadingContent = { AppIcon(rule.packageName) },
                headlineContent = { Text(rule.appLabel.ifBlank { rule.packageName }) },
                supportingContent = { Text(rule.packageName) },
                trailingContent = { TextButton(onClick = onPickApp) { Text("바꾸기") } },
            )
        }
    }
    OutlinedTextField(
        value = rule.titleContains,
        onValueChange = { v -> vm.update { it.copy(titleContains = v) } },
        label = { Text("알림 제목에 포함 (선택)") },
        supportingText = { Text("카카오톡·문자 앱처럼 여러 곳에서 알림이 오는 앱은 보낸 사람 이름을 넣으세요. 예: 신한카드") },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
}

@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
@Composable
private fun FormatSection(vm: RuleEditViewModel) {
    val rule = vm.rule
    val error = remember(rule.parseMode, rule.format) { RuleMatcher.formatError(rule) }
    SectionTitle(if (rule.source == SourceType.SMS) "문자 형식" else "알림 형식")
    val modes = listOf(ParseMode.TEMPLATE to "템플릿", ParseMode.REGEX to "정규식", ParseMode.SERVER to "서버 해석")
    SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
        modes.forEachIndexed { i, (mode, label) ->
            SegmentedButton(
                selected = rule.parseMode == mode,
                onClick = { vm.update { it.copy(parseMode = mode) } },
                shape = SegmentedButtonDefaults.itemShape(i, modes.size),
            ) { Text(label) }
        }
    }
    val help = MaterialTheme.typography.bodySmall
    val helpColor = MaterialTheme.colorScheme.onSurfaceVariant
    when (rule.parseMode) {
        ParseMode.TEMPLATE -> {
            Text(
                "샘플 문자를 붙여 넣고 바뀌는 부분을 항목으로 바꾸세요. 공백·줄바꿈은 있어도 없어도 맞고, {*}는 아무 글자나 건너뜁니다." +
                    (if (rule.source == SourceType.NOTIFICATION) " 알림은 \"제목\\n본문\"으로 맞춰 봅니다." else ""),
                style = help,
                color = helpColor,
            )
            FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                (Field.entries.map { it.placeholder } + WILDCARD).forEach { token ->
                    AssistChip(onClick = { vm.insert(token) }, label = { Text(token) })
                }
            }
            OutlinedTextField(
                value = vm.format,
                onValueChange = vm::changeFormat,
                label = { Text("형식") },
                placeholder = { Text("{카드}({끝자리})승인 {*} {금액}원 {할부} {날짜} {시각} {가맹점} 누적{*}") },
                textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                isError = error != null,
                supportingText = error?.let { { Text(it) } },
                minLines = 3,
                modifier = Modifier.fillMaxWidth(),
            )
            TextButton(
                enabled = rule.sample.isNotBlank(),
                onClick = { vm.changeFormat(TextFieldValue(rule.sample, TextRange(rule.sample.length))) },
            ) { Text("시험 샘플을 형식 칸에 복사") }
        }
        ParseMode.REGEX -> {
            Text(
                "Java 정규식. 이름 있는 그룹으로 값을 뽑습니다: (?<amount>…) 금액, merchant 가맹점, date 날짜, time 시각, " +
                    "installment 할부, card 카드, last4 끝자리. 일부만 맞아도 됩니다.",
                style = help,
                color = helpColor,
            )
            OutlinedTextField(
                value = vm.format,
                onValueChange = vm::changeFormat,
                label = { Text("정규식") },
                placeholder = { Text("""(?<amount>[\d,]+)원.*?(?<date>\d{2}/\d{2})""") },
                textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                isError = error != null,
                supportingText = error?.let { { Text(it) } },
                minLines = 2,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        ParseMode.SERVER -> Text(
            "원문을 그대로 보내고 가계부 서버의 카드 승인 문자 해석(금액·가맹점·일시·할부·취소·카드사·끝자리)을 씁니다. " +
                "일반적인 카드사 형식이면 이것으로 충분합니다. 광고 문자도 금액이 있으면 거래가 되므로 '포함할 단어'(예: 승인)로 거르세요.",
            style = help,
            color = helpColor,
        )
    }
    if (rule.parseMode != ParseMode.SERVER) {
        CheckRow(
            checked = rule.sendRawIfUnmatched,
            onChange = { on -> vm.update { it.copy(sendRawIfUnmatched = on) } },
            title = "형식이 맞지 않아도 원문 보내기",
            detail = "끄면 맞지 않는 문자는 보내지 않고 기록에 '건너뜀'으로 남깁니다.",
        )
    }
    CheckRow(
        checked = rule.cancel,
        onChange = { on -> vm.update { it.copy(cancel = on) } },
        title = "취소/환불 문자 형식",
        detail = "이 규칙으로 보낸 금액은 음수(환불)로 저장됩니다. 승인 형식보다 위에 두세요.",
    )
}

private fun paymentKindLabel(kind: String): String = when (kind) {
    "CREDIT_CARD" -> "신용카드"
    "CHECK_CARD" -> "체크카드"
    "CASH" -> "현금"
    "BANK_TRANSFER" -> "계좌이체"
    else -> "기타"
}

private fun ServerOptions.paymentChoices(): List<Choice> = paymentMethods.map { p ->
    // "현금 · 현금"처럼 이름과 같은 설명은 빼고 보인다
    val detail = listOfNotNull(paymentKindLabel(p.kind), p.issuer, p.last4?.let { "끝자리 $it" }).filterNot { sameName(it, p.name) }
    Choice(p.name, detail = detail.joinToString(" · ").ifEmpty { null })
}

@Composable
private fun ServerValuesSection(vm: RuleEditViewModel, test: TestResult) {
    val rule = vm.rule
    val options = vm.options
    Row(verticalAlignment = Alignment.CenterVertically) {
        SectionTitle("서버로 보낼 값 (선택)", Modifier.weight(1f))
        if (vm.optionsLoading) {
            CircularProgressIndicator(Modifier.padding(12.dp).size(24.dp), strokeWidth = 2.dp)
        } else {
            IconButton(onClick = vm::reloadOptions) { Icon(Icons.Default.Refresh, contentDescription = "가계부 목록 다시 받기") }
        }
    }
    val error = vm.optionsError
    val status = when {
        error != null && options != null -> "목록을 새로 받지 못해 ${formatTime(options.fetchedAt)}에 받은 목록을 씁니다. $error"
        error != null -> "$error. 그동안은 이름을 직접 입력하세요."
        options == null && vm.optionsLoading -> "가계부에서 목록을 받는 중…"
        else -> null
    }
    status?.let {
        Text(
            it,
            style = MaterialTheme.typography.bodySmall,
            color = if (error != null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }

    val parsesCard = rule.parseMode != ParseMode.SERVER
    val paymentChoices = options?.paymentChoices()
    ChoiceField(
        label = if (parsesCard && rule.cardMappings.isNotEmpty()) "기본 결제수단" else "결제수단",
        value = rule.card,
        choices = paymentChoices,
        emptyLabel = if (parsesCard) "자동 ({끝자리}·{카드} 값 → 원문)" else "자동 (서버가 원문에서 찾음)",
        onValueChange = { v -> vm.update { it.copy(card = v) } },
        supportingText = when {
            options == null -> "가계부의 결제수단 이름·별칭 또는 카드 끝 4자리. 비우면 서버가 찾습니다."
            parsesCard && rule.cardMappings.isNotEmpty() -> "아래 카드별 결제수단에 맞는 줄이 없을 때 씁니다."
            else -> null
        },
        modifier = Modifier.fillMaxWidth(),
    )
    if (parsesCard) CardMappingSection(vm, paymentChoices, test)
    LedgerCategoryFields(vm, options)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun CardMappingSection(vm: RuleEditViewModel, paymentChoices: List<Choice>?, test: TestResult) {
    val rule = vm.rule
    val help = MaterialTheme.typography.bodySmall
    Text("카드별 결제수단", style = MaterialTheme.typography.titleSmall)
    Text(
        "같은 형식으로 여러 카드의 문자가 오면, 형식에서 뽑은 {카드}·{끝자리} 값에 따라 결제수단을 고릅니다. " +
            "그 값에 '카드 값'이 들어 있으면 맞고(공백·대소문자 무시), 여러 줄이 맞으면 더 긴 카드 값의 줄을 씁니다.",
        style = help,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    if (rule.cardMappings.isNotEmpty() && !RuleMatcher.capturesCard(rule)) {
        Text("형식에 {카드}나 {끝자리}가 없어 이 목록을 쓰지 않습니다.", style = help, color = MaterialTheme.colorScheme.error)
    }
    val matched = test.card?.mapping
    rule.cardMappings.forEachIndexed { i, m ->
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = m.match,
                onValueChange = { v -> vm.updateMapping(i) { it.copy(match = v) } },
                label = { Text("카드 값") },
                placeholder = { Text("예: 1234") },
                supportingText = if (m === matched) ({ Text("샘플과 맞음", color = MaterialTheme.colorScheme.primary) }) else null,
                singleLine = true,
                modifier = Modifier.weight(1f),
            )
            ChoiceField(
                label = "결제수단",
                value = m.card,
                choices = paymentChoices,
                emptyLabel = "고르세요",
                onValueChange = { v -> vm.updateMapping(i) { it.copy(card = v) } },
                allowEmpty = false,
                matchAnchorWidth = false,
                modifier = Modifier.weight(1.4f),
            )
            IconButton(onClick = { vm.removeMapping(i) }, modifier = Modifier.padding(top = 8.dp)) {
                Icon(Icons.Default.Close, contentDescription = "이 줄 지우기")
            }
        }
    }
    // 시험 샘플에서 뽑은 카드 값을 바로 줄로 추가 (이미 맞는 줄이 있으면 보이지 않음)
    val parsed = test.parsed
    val suggestions = if (matched != null || parsed == null) {
        emptyList()
    } else {
        listOfNotNull(parsed.card, parsed.last4).filter { v -> rule.cardMappings.none { sameName(it.match, v) } }.distinct()
    }
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.Center) {
        TextButton(onClick = { vm.addMapping() }) {
            Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(4.dp))
            Text("줄 추가")
        }
        suggestions.forEach { v ->
            AssistChip(
                onClick = { vm.addMapping(v) },
                label = { Text("샘플의 '$v' 추가") },
                leadingIcon = { Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.size(18.dp)) },
            )
        }
    }
}

@Composable
private fun LedgerCategoryFields(vm: RuleEditViewModel, options: ServerOptions?) {
    val rule = vm.rule
    val defaultLedger = options?.let { o -> (o.ledgers.firstOrNull { it.isDefault } ?: o.ledgers.firstOrNull())?.name }
    val ledgerChoices = options?.ledgers?.map { Choice(it.name, detail = if (it.isDefault) "기본 장부" else null) }
    val ledgerFilter = rule.ledger.trim().takeIf(String::isNotEmpty)
    val categoryChoices = options?.let { o ->
        val list = o.categories.filter { ledgerFilter == null || sameName(it.ledger, ledgerFilter) }
        val grouped = list.map { it.ledger }.distinct().size > 1
        list.map { c ->
            Choice(c.name, detail = if (c.kind == "INCOME") "수입" else null, group = if (grouped) c.ledger else null, tag = c.ledger)
        }
    }
    // 장부를 비우면 서버는 이름이 같은 카테고리 중 기본 장부의 것을 먼저 쓴다
    val selectedCategory = categoryChoices?.let { list ->
        list.firstOrNull { sameName(it.value, rule.category) && (ledgerFilter != null || sameName(it.tag, defaultLedger)) }
            ?: list.firstOrNull { sameName(it.value, rule.category) }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        ChoiceField(
            label = "장부",
            value = rule.ledger,
            choices = ledgerChoices,
            emptyLabel = "기본 장부",
            onValueChange = { v -> vm.update { it.copy(ledger = v) } },
            onPick = { picked ->
                vm.update { r ->
                    val ledger = picked?.value.orEmpty()
                    // 고른 장부에 없는 카테고리는 비운다 (그대로 보내면 서버가 카테고리를 비우고 경고한다)
                    val keep = r.category.isBlank() || ledger.isEmpty() ||
                        options?.categories?.any { sameName(it.ledger, ledger) && sameName(it.name, r.category) } != false
                    r.copy(ledger = ledger, category = if (keep) r.category else "")
                }
            },
            modifier = Modifier.weight(1f),
        )
        ChoiceField(
            label = "카테고리",
            value = rule.category,
            choices = categoryChoices,
            selected = selectedCategory,
            emptyLabel = "자동 추천",
            supportingText = if (rule.category.isBlank()) "같은 가맹점의 과거 내역으로" else null,
            onValueChange = { v -> vm.update { it.copy(category = v) } },
            onPick = { picked ->
                vm.update { r ->
                    // 기본 장부가 아닌 장부의 카테고리를 고르면 장부도 같이 정한다
                    val ledger = picked?.tag
                    r.copy(
                        category = picked?.value.orEmpty(),
                        ledger = if (r.ledger.isBlank() && ledger != null && !sameName(ledger, defaultLedger)) ledger else r.ledger,
                    )
                }
            },
            matchAnchorWidth = false,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun CheckRow(checked: Boolean, onChange: (Boolean) -> Unit, title: String, detail: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Checkbox(checked = checked, onCheckedChange = onChange)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyMedium)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

private val prettyJson = Json { prettyPrint = true }

@Composable
private fun TestSection(vm: RuleEditViewModel, result: TestResult, onPickSample: () -> Unit) {
    val rule = vm.rule
    SectionTitle("시험")
    OutlinedTextField(
        value = rule.sample,
        onValueChange = { v -> vm.update { it.copy(sample = v) } },
        label = { Text(if (rule.source == SourceType.SMS) "샘플 문자" else "샘플 알림 (첫 줄 = 제목)") },
        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
        minLines = 3,
        modifier = Modifier.fillMaxWidth(),
    )
    OutlinedButton(onClick = onPickSample, modifier = Modifier.fillMaxWidth()) {
        Text(if (rule.source == SourceType.SMS) "받은 문자에서 가져오기" else "최근 알림에서 가져오기")
    }
    TestResultCard(rule, result)
}

@Composable
private fun TestResultCard(rule: Rule, result: TestResult) {
    if (result.formatError != null) return
    val ok = MaterialTheme.colorScheme.primary
    val bad = MaterialTheme.colorScheme.error
    OutlinedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            when {
                rule.sample.isBlank() -> Text("샘플을 넣으면 뽑히는 값과 보낼 내용을 미리 봅니다.", style = MaterialTheme.typography.bodySmall)
                result.titleMissing -> Text("첫 줄(제목)에 '${rule.titleContains.trim()}'가 없어 처리하지 않습니다.", color = bad)
                result.keywordMissing -> Text("포함할 단어가 없어 처리하지 않습니다.", color = bad)
                rule.parseMode == ParseMode.SERVER -> Text("원문을 보내 서버가 해석합니다.", color = ok)
                result.parsed != null -> {
                    Text("형식과 맞습니다", color = ok, style = MaterialTheme.typography.titleSmall)
                    val p = result.parsed
                    listOfNotNull(
                        p.amount?.let { "금액" to formatWon(it) },
                        p.merchant?.let { "가맹점" to it },
                        p.date?.let { "날짜" to it.toString() },
                        p.time?.let { "시각" to it },
                        p.installmentMonths?.let { "할부" to if (it == 1) "일시불" else "${it}개월" },
                        p.card?.let { "카드" to it },
                        p.last4?.let { "끝자리" to it },
                    ).forEach { (k, v) ->
                        Row {
                            Text(k, Modifier.padding(end = 12.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(v)
                        }
                    }
                    if (p.amount == null) Text("금액을 뽑지 못해 서버가 원문에서 찾습니다.", style = MaterialTheme.typography.bodySmall)
                }
                rule.sendRawIfUnmatched -> Text("형식과 맞지 않습니다 — 원문만 보내 서버가 해석합니다.", color = bad)
                else -> Text("형식과 맞지 않습니다 — 보내지 않고 기록에 '건너뜀'으로 남깁니다.", color = bad)
            }
            result.card?.let { c ->
                val from = when (c.source) {
                    CardSource.MAPPING -> "카드별 '${c.mapping?.match?.trim()}'"
                    CardSource.RULE -> "규칙"
                    CardSource.LAST4 -> "{끝자리}"
                    CardSource.CARD -> "{카드}"
                }
                Row {
                    Text("결제수단", Modifier.padding(end = 12.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text("${c.value} ($from)")
                }
            }
            result.payload?.let { payload: JsonObject ->
                Spacer(Modifier.height(4.dp))
                Text("보낼 내용", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(
                    prettyJson.encodeToString(JsonObject.serializer(), payload),
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                )
            }
        }
    }
}
