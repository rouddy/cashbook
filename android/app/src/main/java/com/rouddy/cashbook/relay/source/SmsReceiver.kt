package com.rouddy.cashbook.relay.source

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.provider.Telephony
import com.rouddy.cashbook.relay.Graph
import com.rouddy.cashbook.relay.data.SourceType
import com.rouddy.cashbook.relay.ingest.IncomingMessage
import com.rouddy.cashbook.relay.ingest.Ingestor
import kotlinx.coroutines.launch

/** 단문(SMS) 수신. 여러 조각으로 온 긴 SMS는 보낸 번호별로 이어 붙인다. */
class SmsReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION) return
        val parts = Telephony.Sms.Intents.getMessagesFromIntent(intent)?.filterNotNull().orEmpty()
        if (parts.isEmpty()) return
        val messages = parts
            .groupBy { it.originatingAddress ?: it.displayOriginatingAddress.orEmpty() }
            .map { (sender, pieces) ->
                IncomingMessage(
                    source = SourceType.SMS,
                    origin = sender,
                    body = pieces.joinToString("") { it.messageBody.orEmpty() },
                    receivedAt = pieces.first().timestampMillis.takeIf { it > 0 } ?: System.currentTimeMillis(),
                )
            }
        val app = context.applicationContext
        val pending = goAsync()
        Graph.scope.launch {
            try {
                messages.forEach { Ingestor.handle(app, it) }
            } finally {
                pending.finish()
            }
        }
    }
}
