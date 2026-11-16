package com.rouddy.cashbook.relay.data

import android.content.Context
import androidx.datastore.core.CorruptionException
import androidx.datastore.core.DataStore
import androidx.datastore.core.DataStoreFactory
import androidx.datastore.core.Serializer
import androidx.datastore.core.handlers.ReplaceFileCorruptionHandler
import androidx.datastore.dataStoreFile
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import java.io.InputStream
import java.io.OutputStream

val AppJson = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
}

private class JsonFileSerializer<T>(
    private val serializer: KSerializer<T>,
    override val defaultValue: T,
) : Serializer<T> {
    override suspend fun readFrom(input: InputStream): T = try {
        AppJson.decodeFromString(serializer, input.readBytes().decodeToString())
    } catch (e: SerializationException) {
        throw CorruptionException("저장 파일을 읽지 못했습니다", e)
    } catch (e: IllegalArgumentException) {
        throw CorruptionException("저장 파일을 읽지 못했습니다", e)
    }

    override suspend fun writeTo(t: T, output: OutputStream) {
        output.write(AppJson.encodeToString(serializer, t).encodeToByteArray())
    }
}

/**
 * 설정·규칙(config.json)과 업로드 기록(history.json). 한 프로세스에 하나만 만든다 ([com.rouddy.cashbook.relay.Graph]).
 */
class Store(context: Context) {
    private val config: DataStore<AppConfig> = DataStoreFactory.create(
        serializer = JsonFileSerializer(AppConfig.serializer(), AppConfig()),
        corruptionHandler = ReplaceFileCorruptionHandler { AppConfig() },
        produceFile = { context.dataStoreFile("config.json") },
    )
    private val history: DataStore<History> = DataStoreFactory.create(
        serializer = JsonFileSerializer(History.serializer(), History()),
        corruptionHandler = ReplaceFileCorruptionHandler { History() },
        produceFile = { context.dataStoreFile("history.json") },
    )

    val configFlow: Flow<AppConfig> = config.data
    val recordsFlow: Flow<List<UploadRecord>> = history.data.map { it.records }

    suspend fun config(): AppConfig = config.data.first()

    suspend fun updateSettings(transform: (Settings) -> Settings) {
        config.updateData { it.copy(settings = transform(it.settings)) }
    }

    /** 있으면 같은 자리에서 바꾸고, 없으면 끝에 추가 */
    suspend fun saveRule(rule: Rule) {
        config.updateData { c ->
            val exists = c.rules.any { it.id == rule.id }
            c.copy(rules = if (exists) c.rules.map { if (it.id == rule.id) rule else it } else c.rules + rule)
        }
    }

    suspend fun deleteRule(id: String) {
        config.updateData { c -> c.copy(rules = c.rules.filterNot { it.id == id }) }
    }

    suspend fun setRuleEnabled(id: String, enabled: Boolean) {
        config.updateData { c -> c.copy(rules = c.rules.map { if (it.id == id) it.copy(enabled = enabled) else it }) }
    }

    /** 같은 출처의 규칙끼리 순서를 바꾼다 (delta = -1 위로, +1 아래로) */
    suspend fun moveRule(id: String, delta: Int) {
        config.updateData { c ->
            val rule = c.rules.firstOrNull { it.id == id } ?: return@updateData c
            val sameSource = c.rules.withIndex().filter { it.value.source == rule.source }
            val pos = sameSource.indexOfFirst { it.value.id == id }
            val target = sameSource.getOrNull(pos + delta) ?: return@updateData c
            val list = c.rules.toMutableList()
            val from = sameSource[pos].index
            list[from] = list[target.index].also { list[target.index] = list[from] }
            c.copy(rules = list)
        }
    }

    suspend fun setMmsLastId(id: Long) {
        config.updateData { it.copy(mmsLastId = id) }
    }

    /** 같은 원문(dedupeKey)이 이미 있으면 추가하지 않고 false */
    suspend fun addRecord(record: UploadRecord): Boolean {
        var added = false
        history.updateData { h ->
            if (h.records.any { it.dedupeKey == record.dedupeKey }) {
                added = false
                h
            } else {
                added = true
                h.copy(records = trim(listOf(record) + h.records))
            }
        }
        return added
    }

    suspend fun records(): List<UploadRecord> = history.data.first().records

    suspend fun record(id: String): UploadRecord? = records().firstOrNull { it.id == id }

    suspend fun updateRecord(id: String, transform: (UploadRecord) -> UploadRecord): UploadRecord? {
        var updated: UploadRecord? = null
        history.updateData { h ->
            h.copy(
                records = h.records.map {
                    if (it.id == id) transform(it).copy(updatedAt = System.currentTimeMillis()).also { r -> updated = r } else it
                },
            )
        }
        return updated
    }

    suspend fun deleteRecord(id: String) {
        history.updateData { h -> h.copy(records = h.records.filterNot { it.id == id }) }
    }

    /** 대기 중인 기록은 남긴다 */
    suspend fun clearFinishedRecords() {
        history.updateData { h -> h.copy(records = h.records.filter { it.status == UploadStatus.PENDING }) }
    }

    companion object {
        const val MAX_RECORDS = 300
        const val MAX_SKIPPED = 30

        /**
         * 최신순 목록을 자른다: 대기 중인 기록은 항상 남기고, 나머지는 최근 [MAX_RECORDS]개
         * (그중 건너뜀은 최근 [MAX_SKIPPED]개까지).
         */
        internal fun trim(records: List<UploadRecord>): List<UploadRecord> {
            var kept = 0
            var skipped = 0
            return records.filter { r ->
                when (r.status) {
                    UploadStatus.PENDING -> true
                    UploadStatus.SKIPPED -> ++skipped <= MAX_SKIPPED && ++kept <= MAX_RECORDS
                    else -> ++kept <= MAX_RECORDS
                }
            }
        }
    }
}
