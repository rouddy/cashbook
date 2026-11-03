package com.rouddy.cashbook.relay.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.UploadStatus
import com.rouddy.cashbook.relay.ingest.CashbookApi
import com.rouddy.cashbook.relay.ingest.Ingestor
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }
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

    suspend fun save() {
        Graph.store.updateSettings { it.copy(serverUrl = url.trim().trimEnd('/'), token = token.trim()) }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("서버 설정") },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "뒤로") } },
                actions = {
                    TextButton(enabled = loaded, onClick = {
                        scope.launch {
                            save()
                            val failed = Graph.store.records().count { it.status == UploadStatus.FAILED }
                            if (failed > 0) {
                                val action = snackbar.showSnackbar("저장했습니다. 실패한 ${failed}건이 있습니다", actionLabel = "다시 보내기")
                                if (action == androidx.compose.material3.SnackbarResult.ActionPerformed) Ingestor.resendFailed(context)
                            } else {
                                snackbar.showSnackbar("저장했습니다")
                            }
                        }
                    }) { Text("저장") }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { padding ->
        if (!loaded) return@Scaffold
        Column(
            Modifier.fillMaxSize().padding(padding).imePadding().verticalScroll(rememberScrollState()).padding(16.dp),
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
                supportingText = { Text("가계부 설정 → 자동 입력 API 토큰에서 발급 (자동 입력 전용 권한이면 충분). 서버에 비밀번호가 없으면 비워 둡니다.") },
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
                OutlinedButton(
                    enabled = !checking,
                    onClick = {
                        scope.launch {
                            save()
                            snackbar.showSnackbar("저장했습니다")
                        }
                    },
                ) { Text("저장") }
                if (checking) CircularProgressIndicator(Modifier.size(24.dp))
            }
            checkResult?.let { (ok, message) ->
                Text(message, color = if (ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error)
            }
            Row(verticalAlignment = Alignment.Top) {
                Icon(Icons.Default.Lock, contentDescription = null, tint = MaterialTheme.colorScheme.outline, modifier = Modifier.padding(top = 2.dp).size(16.dp))
                Text(
                    "  토큰과 받은 문자 기록은 이 기기 앱 저장소에만 두고 백업에서 제외합니다. " +
                        "연결 확인은 빈 요청을 자동 입력 API로 보내 인증만 확인하며 거래를 만들지 않습니다.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
