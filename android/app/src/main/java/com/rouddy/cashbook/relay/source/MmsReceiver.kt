package com.rouddy.cashbook.relay.source

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.provider.Telephony

/**
 * 장문(LMS/MMS) 수신 알림. `[Web발신]` 카드 승인 문자는 90바이트를 넘으면 LMS로 와서 SMS 방송이 오지 않는다.
 * 이 시점에는 본문이 아직 내려받아지지 않았으므로, 기본 문자 앱이 받아 저장한 뒤 [MmsWorker]가 읽는다.
 */
class MmsReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.WAP_PUSH_RECEIVED_ACTION) return
        if (intent.type != "application/vnd.wap.mms-message") return
        MmsWorker.enqueue(context, delaySeconds = 5)
    }
}
