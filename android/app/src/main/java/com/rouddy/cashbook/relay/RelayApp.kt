package com.rouddy.cashbook.relay

import android.app.Application
import android.content.Context
import com.rouddy.cashbook.relay.book.BookRepository
import com.rouddy.cashbook.relay.data.Store
import com.rouddy.cashbook.relay.data.UiPrefs
import com.rouddy.cashbook.relay.ingest.Notifications
import com.rouddy.cashbook.relay.source.MmsWorker
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/** 앱 전체에서 하나만 쓰는 객체 */
object Graph {
    lateinit var store: Store
        private set

    /** 가계부 GraphQL (대시보드·수동 입력·최근 내역) */
    lateinit var book: BookRepository
        private set

    lateinit var prefs: UiPrefs
        private set

    /** 수신기·알림 서비스에서 화면과 관계없이 처리할 때 쓰는 범위 */
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    fun init(context: Context) {
        if (!::store.isInitialized) {
            store = Store(context.applicationContext)
            book = BookRepository(store)
            prefs = UiPrefs(context.applicationContext)
        }
    }
}

class RelayApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Graph.init(this)
        Notifications.createChannels(this)
        Graph.scope.launch {
            // 장문 기준점을 잡고, 프로세스가 꺼져 있던 동안 받은 장문이 있으면 처리한다
            MmsWorker.initCursor(this@RelayApp)
            MmsWorker.enqueue(this@RelayApp)
        }
    }
}
