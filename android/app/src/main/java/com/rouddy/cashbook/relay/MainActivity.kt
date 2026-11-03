package com.rouddy.cashbook.relay

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.mutableIntStateOf
import com.rouddy.cashbook.relay.ingest.Notifications
import com.rouddy.cashbook.relay.ui.AppNav
import com.rouddy.cashbook.relay.ui.theme.RelayTheme

class MainActivity : ComponentActivity() {
    /** 실패 알림을 눌러 들어오면 기록 화면을 연다 (값이 바뀔 때마다 한 번) */
    private val openHistory = mutableIntStateOf(0)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) handle(intent)
        setContent {
            RelayTheme {
                AppNav(openHistoryRequest = openHistory.intValue)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        if (intent?.getBooleanExtra(Notifications.EXTRA_OPEN_HISTORY, false) == true) openHistory.intValue++
    }
}
