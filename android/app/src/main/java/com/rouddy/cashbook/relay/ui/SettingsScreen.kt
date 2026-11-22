package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.AppConfig
import com.rouddy.cashbook.relay.data.Rule
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.data.UploadStatus
import com.rouddy.cashbook.relay.ingest.CashbookApi
import com.rouddy.cashbook.relay.ingest.Ingestor
import kotlinx.coroutines.launch

/** 설정 화면의 탭 (예전 첫 화면의 내용을 나눠 담는다) */
enum class SettingsTab(val label: String) {
    SERVER("서버"),
    PERMISSIONS("권한"),
    RULES("규칙"),
    HISTORY("업로드 기록"),
}

/** 설정 탭·하단 메뉴에 띄울 알림 표시 */
data class SettingsAlerts(val serverMissing: Boolean, val permissionMissing: Boolean, val failed: Int) {
    val any: Boolean get() = serverMissing || permissionMissing || failed > 0
}

@Composable
fun rememberSettingsAlerts(config: AppConfig?): SettingsAlerts {
    val records by Graph.store.recordsFlow.collectAsStateWithLifecycle(initialValue = emptyList())
    val (permissions, _) = rememberPermissionState()
    return SettingsAlerts(
        serverMissing = config != null && config.settings.serverUrl.isBlank(),
        permissionMissing = PermissionNeeds.of(config).missing(permissions),
        failed = records.count { it.status == UploadStatus.FAILED },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    tab: SettingsTab,
    onTabChange: (SettingsTab) -> Unit,
    onEditRule: (Rule) -> Unit,
    onAddRule: (SourceType) -> Unit,
) {
    val snackbar = remember { SnackbarHostState() }
    val config by Graph.store.configFlow.collectAsStateWithLifecycle(initialValue = null)
    val alerts = rememberSettingsAlerts(config)
    // 탭을 오가도 입력 중인 서버 주소·스크롤 위치를 잃지 않게 탭마다 상태를 따로 둔다
    val tabState = rememberSaveableStateHolder()

    Scaffold(
        topBar = {
            Column {
                TopAppBar(
                    title = { Text("설정") },
                    actions = { if (tab == SettingsTab.HISTORY) HistoryMenu(snackbar) },
                )
                PrimaryTabRow(selectedTabIndex = tab.ordinal) {
                    SettingsTab.entries.forEach { t ->
                        Tab(
                            selected = t == tab,
                            onClick = { onTabChange(t) },
                            text = {
                                val badge = when (t) {
                                    SettingsTab.SERVER -> alerts.serverMissing
                                    SettingsTab.PERMISSIONS -> alerts.permissionMissing
                                    SettingsTab.HISTORY -> alerts.failed > 0
                                    SettingsTab.RULES -> false
                                }
                                BadgedBox(
                                    badge = {
                                        if (t == SettingsTab.HISTORY && alerts.failed > 0) {
                                            Badge { Text("${alerts.failed}") }
                                        } else if (badge) {
                                            Badge()
                                        }
                                    },
                                ) { Text(t.label, maxLines = 1) }
                            },
                        )
                    }
                }
            }
        },
        floatingActionButton = { if (tab == SettingsTab.RULES) AddRuleButton(onAddRule) },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        tabState.SaveableStateProvider(tab.name) {
            when (tab) {
                SettingsTab.SERVER -> ServerPane(padding, snackbar)
                SettingsTab.PERMISSIONS -> PermissionsPane(padding, config)
                SettingsTab.RULES -> RulesPane(padding, onEditRule)
                SettingsTab.HISTORY -> HistoryPane(padding)
            }
        }
    }
}

/** 설정 → 서버: 가계부 주소와 API 토큰 */
@Composable
private fun ServerPane(contentPadding: PaddingValues, snackbar: SnackbarHostState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var loaded by rememberSaveable { mutableStateOf(false) }
    var url by rememberSaveable { mutableStateOf("") }
    var token by rememberSaveable { mutableStateOf("") }
    var showToken by rememberSaveable { mutableStateOf(false) }
    var checking by remember { mutableStateOf(false) }
    var checkResult by remember { mutableStateOf<Pair<Boolean, String>?>(null) }

    LaunchedEffect(Unit) {
        if (!loaded) {
            val settings = Graph.store.config().settings
            url = settings.serverUrl
            token = settings.token
            loaded = true
        }
    }

    fun save() = scope.launch {
        val before = Graph.store.config().settings
        val newUrl = url.trim().trimEnd('/')
        val newToken = token.trim()
        Graph.store.updateSettings { it.copy(serverUrl = newUrl, token = newToken) }
        // 대시보드·수동 입력·최근 내역이 새 서버(토큰)로 다시 읽는다
        if (before.serverUrl != newUrl || before.token != newToken) Graph.book.reset()
        val failed = Graph.store.records().count { it.status == UploadStatus.FAILED }
        if (failed > 0) {
            val action = snackbar.showSnackbar("저장했습니다. 실패한 ${failed}건이 있습니다", actionLabel = "다시 보내기")
            if (action == SnackbarResult.ActionPerformed) Ingestor.resendFailed(context)
        } else {
            snackbar.showSnackbar("저장했습니다")
        }
    }

    if (!loaded) return
    Column(
        Modifier.fillMaxSize().padding(contentPadding).imePadding().verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        OutlinedTextField(
            value = url,
            onValueChange = {
                url = it
                checkResult = null
            },
            label = { Text("서버 주소") },
            placeholder = { Text("https://cash.example.com") },
            supportingText = { Text("가계부 웹 주소. http는 localhost(adb reverse)·에뮬레이터(10.0.2.2)만 됩니다.") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = token,
            onValueChange = {
                token = it
                checkResult = null
            },
            label = { Text("API 토큰") },
            placeholder = { Text("cb_...") },
            supportingText = {
                Text(
                    "가계부 웹 설정 → API 토큰에서 발급합니다. 자동 입력과 대시보드·수동 입력·최근 내역에 같은 토큰을 씁니다. " +
                        "서버에 비밀번호가 없으면 비워 둡니다.",
                )
            },
            singleLine = true,
            visualTransformation = if (showToken) VisualTransformation.None else PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            trailingIcon = {
                TextButton(onClick = { showToken = !showToken }) { Text(if (showToken) "숨기기" else "보기") }
            },
            modifier = Modifier.fillMaxWidth(),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(
                enabled = !checking && url.isNotBlank(),
                onClick = {
                    scope.launch {
                        checking = true
                        checkResult = CashbookApi(url, token).check()
                        checking = false
                    }
                },
            ) { Text("연결 확인") }
            OutlinedButton(enabled = !checking, onClick = { save() }) { Text("저장") }
            if (checking) CircularProgressIndicator(Modifier.size(24.dp))
        }
        checkResult?.let { (ok, message) ->
            Text(message, color = if (ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error)
        }
        Row(verticalAlignment = Alignment.Top) {
            Icon(Icons.Default.Lock, contentDescription = null, tint = MaterialTheme.colorScheme.outline, modifier = Modifier.padding(top = 2.dp).size(16.dp))
            Text(
                "  토큰과 받은 문자 기록은 이 기기 앱 저장소에만 두고 백업에서 제외합니다. " +
                    "연결 확인은 빈 요청을 자동 입력 API로 보내 인증을 확인하고, GraphQL 읽기도 되는지 봅니다. 거래는 만들지 않습니다.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
