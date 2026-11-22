package com.rouddy.cashbook.relay.data

import android.content.Context
import androidx.core.content.edit

/** 화면에서 마지막으로 고른 값 (SharedPreferences). 규칙·설정은 [Store]에 둔다. */
class UiPrefs(context: Context) {
    private val prefs = context.getSharedPreferences("ui_prefs", Context.MODE_PRIVATE)

    /** 최근 내역의 "자동 입력만" 체크 */
    var recentAutoOnly: Boolean
        get() = prefs.getBoolean(KEY_RECENT_AUTO_ONLY, false)
        set(value) = prefs.edit { putBoolean(KEY_RECENT_AUTO_ONLY, value) }

    /** 최근 내역의 "미분류만" 체크 */
    var recentUncategorizedOnly: Boolean
        get() = prefs.getBoolean(KEY_RECENT_UNCATEGORIZED_ONLY, false)
        set(value) = prefs.edit { putBoolean(KEY_RECENT_UNCATEGORIZED_ONLY, value) }

    /** 대시보드 장부: [LEDGER_DEFAULT], [LEDGER_ALL] 또는 장부 id */
    var dashboardLedger: String
        get() = prefs.getString(KEY_DASHBOARD_LEDGER, LEDGER_DEFAULT) ?: LEDGER_DEFAULT
        set(value) = prefs.edit { putString(KEY_DASHBOARD_LEDGER, value) }

    companion object {
        private const val KEY_RECENT_AUTO_ONLY = "recent_auto_only"
        private const val KEY_RECENT_UNCATEGORIZED_ONLY = "recent_uncategorized_only"
        private const val KEY_DASHBOARD_LEDGER = "dashboard_ledger"
        const val LEDGER_DEFAULT = "default"
        const val LEDGER_ALL = "all"
    }
}
