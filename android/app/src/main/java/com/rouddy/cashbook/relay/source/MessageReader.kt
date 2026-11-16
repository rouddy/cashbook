package com.rouddy.cashbook.relay.source

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.provider.BaseColumns
import android.provider.Telephony
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import com.rouddy.cashbook.relay.parse.PhoneNumber

/** 기기의 문자 저장소(받은편지함) 읽기. READ_SMS 권한이 필요하다. */
object MessageReader {
    data class Message(val address: String, val body: String, val date: Long, val mms: Boolean)

    data class Sender(val address: String, val count: Int, val latest: Message)

    data class MmsEntry(val id: Long, val date: Long, val downloaded: Boolean)

    /** PduHeaders.MESSAGE_TYPE_RETRIEVE_CONF (내려받기 끝난 메시지) */
    private const val MESSAGE_TYPE_RETRIEVE_CONF = 132

    /** PduHeaders.FROM */
    private const val ADDRESS_TYPE_FROM = 137

    fun canRead(context: Context): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.READ_SMS) == PackageManager.PERMISSION_GRANTED

    private inline fun <T> Context.query(
        uri: Uri,
        projection: Array<String>,
        selection: String? = null,
        args: Array<String>? = null,
        order: String? = null,
        block: (Cursor) -> T,
    ): T? = try {
        contentResolver.query(uri, projection, selection, args, order)?.use(block)
    } catch (_: SecurityException) {
        null
    } catch (_: IllegalArgumentException) {
        null
    }

    private fun Cursor.str(column: String): String? = getColumnIndex(column).takeIf { it >= 0 }?.let { getString(it) }
    private fun Cursor.long(column: String): Long = getColumnIndex(column).takeIf { it >= 0 }?.let { getLong(it) } ?: 0L
    private fun Cursor.int(column: String): Int = getColumnIndex(column).takeIf { it >= 0 }?.let { getInt(it) } ?: 0

    /** 최근 받은 단문 + 장문 (최신순) */
    fun recentMessages(context: Context, smsLimit: Int = 300, mmsLimit: Int = 40): List<Message> {
        if (!canRead(context)) return emptyList()
        val sms = context.query(
            Telephony.Sms.Inbox.CONTENT_URI,
            arrayOf(Telephony.Sms.ADDRESS, Telephony.Sms.BODY, Telephony.Sms.DATE),
            order = "${Telephony.Sms.DATE} DESC",
        ) { c ->
            buildList {
                while (size < smsLimit && c.moveToNext()) {
                    val address = c.str(Telephony.Sms.ADDRESS) ?: continue
                    add(Message(address, c.str(Telephony.Sms.BODY).orEmpty(), c.long(Telephony.Sms.DATE), mms = false))
                }
            }
        }.orEmpty()
        val mms = context.query(
            Telephony.Mms.Inbox.CONTENT_URI,
            arrayOf(BaseColumns._ID, Telephony.Mms.DATE, Telephony.Mms.MESSAGE_TYPE),
            order = "${BaseColumns._ID} DESC",
        ) { c ->
            buildList {
                while (size < mmsLimit && c.moveToNext()) {
                    add(MmsEntry(c.long(BaseColumns._ID), c.long(Telephony.Mms.DATE) * 1000, c.int(Telephony.Mms.MESSAGE_TYPE) == MESSAGE_TYPE_RETRIEVE_CONF))
                }
            }
        }.orEmpty().filter { it.downloaded }.mapNotNull { mmsMessage(context, it) }
        return (sms + mms).sortedByDescending { it.date }
    }

    /** 보낸 번호별로 묶은 목록 (최근 받은 순) */
    fun senders(context: Context): List<Sender> =
        recentMessages(context)
            .groupBy { PhoneNumber.normalize(it.address) }
            .map { (_, messages) -> Sender(messages.first().address, messages.size, messages.first()) }
            .sortedByDescending { it.latest.date }

    fun messagesFrom(context: Context, senders: List<String>): List<Message> =
        recentMessages(context).filter { m -> senders.any { PhoneNumber.same(it, m.address) } }

    fun mmsMaxId(context: Context): Long = context.query(
        Telephony.Mms.CONTENT_URI,
        arrayOf(BaseColumns._ID),
        order = "${BaseColumns._ID} DESC",
    ) { c -> if (c.moveToFirst()) c.getLong(0) else -1L } ?: -1L

    /** 받은편지함 장문 중 [afterId]보다 뒤 (_id 오름차순) */
    fun mmsInbox(context: Context, afterId: Long, sinceMillis: Long?): List<MmsEntry> {
        val selection = buildString {
            append("${BaseColumns._ID} > ?")
            if (sinceMillis != null) append(" AND ${Telephony.Mms.DATE} >= ?")
        }
        val args = listOfNotNull(afterId.toString(), sinceMillis?.let { (it / 1000).toString() }).toTypedArray()
        return context.query(
            Telephony.Mms.Inbox.CONTENT_URI,
            arrayOf(BaseColumns._ID, Telephony.Mms.DATE, Telephony.Mms.MESSAGE_TYPE),
            selection,
            args,
            "${BaseColumns._ID} ASC",
        ) { c ->
            buildList {
                while (c.moveToNext()) {
                    add(MmsEntry(c.long(BaseColumns._ID), c.long(Telephony.Mms.DATE) * 1000, c.int(Telephony.Mms.MESSAGE_TYPE) == MESSAGE_TYPE_RETRIEVE_CONF))
                }
            }
        }.orEmpty()
    }

    fun mmsMessage(context: Context, entry: MmsEntry): Message? {
        val address = context.query(
            "content://mms/${entry.id}/addr".toUri(),
            arrayOf(Telephony.Mms.Addr.ADDRESS, Telephony.Mms.Addr.TYPE),
            "${Telephony.Mms.Addr.TYPE} = ?",
            arrayOf(ADDRESS_TYPE_FROM.toString()),
        ) { c -> if (c.moveToFirst()) c.str(Telephony.Mms.Addr.ADDRESS) else null } ?: return null
        val body = mmsText(context, entry.id)
        return Message(address, body, entry.date, mms = true)
    }

    private fun mmsText(context: Context, id: Long): String {
        val parts = context.query(
            "content://mms/$id/part".toUri(),
            arrayOf(BaseColumns._ID, Telephony.Mms.Part.CONTENT_TYPE, Telephony.Mms.Part.TEXT, Telephony.Mms.Part._DATA),
        ) { c ->
            buildList {
                while (c.moveToNext()) {
                    if (c.str(Telephony.Mms.Part.CONTENT_TYPE) != "text/plain") continue
                    val inline = c.str(Telephony.Mms.Part.TEXT)
                    add(inline ?: readPart(context, c.long(BaseColumns._ID)))
                }
            }
        }.orEmpty()
        return parts.filterNotNull().joinToString("\n").trim()
    }

    private fun readPart(context: Context, partId: Long): String? = try {
        context.contentResolver.openInputStream("content://mms/part/$partId".toUri())?.use { it.readBytes().decodeToString() }
    } catch (_: Exception) {
        null
    }
}
