package com.rouddy.cashbook.relay.source

import android.app.Notification
import android.content.ComponentName
import android.content.Context
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import androidx.core.app.NotificationManagerCompat
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.ingest.IncomingMessage
import com.rouddy.cashbook.relay.ingest.Ingestor
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** 다른 앱의 알림을 받아 규칙에 넘긴다. 설정 → 알림 접근에서 허용해야 시스템이 연결한다. */
class NotificationCaptureService : NotificationListenerService() {

    override fun onListenerConnected() {
        connected.value = true
    }

    override fun onListenerDisconnected() {
        connected.value = false
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        if (sbn.packageName == packageName) return
        val notification = sbn.notification ?: return
        if (notification.flags and Notification.FLAG_GROUP_SUMMARY != 0) return
        if (sbn.isOngoing) return
        val extras = notification.extras ?: return
        val title = (extras.getCharSequence(Notification.EXTRA_TITLE_BIG) ?: extras.getCharSequence(Notification.EXTRA_TITLE))
            ?.toString()?.trim().orEmpty()
        val body = (extras.getCharSequence(Notification.EXTRA_BIG_TEXT) ?: extras.getCharSequence(Notification.EXTRA_TEXT))
            ?.toString()?.trim().orEmpty()
        if (title.isEmpty() && body.isEmpty()) return

        val message = IncomingMessage(SourceType.NOTIFICATION, sbn.packageName, title, body, sbn.postTime)
        RecentNotifications.add(message)
        val app = applicationContext
        Graph.scope.launch { Ingestor.handle(app, message) }
    }

    companion object {
        /** 이 프로세스에서 시스템과 연결되어 있는지 */
        val connected = MutableStateFlow(false)

        fun isEnabled(context: Context): Boolean =
            NotificationManagerCompat.getEnabledListenerPackages(context).contains(context.packageName)

        fun component(context: Context) = ComponentName(context, NotificationCaptureService::class.java)
    }
}

/**
 * 규칙을 만들 때 샘플로 고를 수 있게 최근 알림을 메모리에만 잠깐 둔다 (파일에 저장하지 않음, 앱을 다시 시작하면 사라짐).
 */
object RecentNotifications {
    private const val LIMIT = 100
    private val _items = MutableStateFlow<List<IncomingMessage>>(emptyList())
    val items: StateFlow<List<IncomingMessage>> = _items.asStateFlow()

    fun add(message: IncomingMessage) {
        _items.update { list ->
            (listOf(message) + list.filterNot { it.origin == message.origin && it.fullText == message.fullText }).take(LIMIT)
        }
    }
}
