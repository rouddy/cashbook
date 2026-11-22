package com.rouddy.cashbook.relay.book

import java.text.NumberFormat
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToLong

// 가계부 화면의 숫자·날짜 표시 (웹 `lib/format.ts`, `lib/transfer.ts`와 같은 규칙)

private val number = NumberFormat.getNumberInstance(Locale.KOREA)
private val dayFormat = DateTimeFormatter.ofPattern("M월 d일 (E)", Locale.KOREAN)
private val shortFormat = DateTimeFormatter.ofPattern("M/d (E)", Locale.KOREAN)

/** 12,500 */
fun amountText(n: Long): String = number.format(n)

fun amountText(n: Double): String = number.format(n.roundToLong())

/** +12,000원 / -3,000원 / 0원 */
fun signedWon(n: Long): String = when {
    n > 0 -> "+${amountText(n)}원"
    n < 0 -> "-${amountText(abs(n))}원"
    else -> "0원"
}

/** 축 눈금용: 0 / 50만 / 1,200만 / 1.2억 */
fun compactWon(n: Long): String {
    val a = abs(n)
    val sign = if (n < 0) "-" else ""
    return when {
        a >= 100_000_000 -> {
            val v = a / 100_000_000.0
            sign + (if (v >= 10) v.roundToLong().toString() else ((v * 10).roundToLong() / 10.0).toString().removeSuffix(".0")) + "억"
        }
        a >= 10_000 -> sign + number.format((a / 10_000.0).roundToLong()) + "만"
        else -> sign + number.format(a)
    }
}

fun parseDate(s: String): LocalDate? = runCatching { LocalDate.parse(s.take(10)) }.getOrNull()

fun parseMonth(s: String): YearMonth? = runCatching { YearMonth.parse(s.take(7)) }.getOrNull()

/** 2026년 11월 */
fun monthTitle(m: YearMonth): String = "${m.year}년 ${m.monthValue}월"

/** 11월 21일 (금) */
fun dayTitle(d: LocalDate): String = d.format(dayFormat)

/** 11/21 (금) */
fun shortDate(s: String): String = parseDate(s)?.format(shortFormat) ?: s

/** 보고 있는 장부 기준 이체 방향 */
enum class TransferDirection { IN, OUT, BETWEEN }

fun transferDirection(tx: Tx, viewLedgerId: Int?): TransferDirection? {
    val to = tx.transferLedgerId ?: return null
    return when {
        viewLedgerId != null && viewLedgerId == to -> TransferDirection.IN
        viewLedgerId != null && viewLedgerId == tx.ledgerId -> TransferDirection.OUT
        else -> TransferDirection.BETWEEN
    }
}

/** 항목 자리에 보일 이체 표시: '→ 용돈', '← 가계부', '가계부 → 용돈' */
fun transferLabel(tx: Tx, ref: RefData?, viewLedgerId: Int?): String? {
    val dir = transferDirection(tx, viewLedgerId) ?: return null
    val from = ref?.ledger(tx.ledgerId)?.name ?: "?"
    val to = ref?.ledger(tx.transferLedgerId)?.name ?: "?"
    return when (dir) {
        TransferDirection.IN -> "← $from"
        TransferDirection.OUT -> "→ $to"
        TransferDirection.BETWEEN -> "$from → $to"
    }
}

/** 들어온 돈으로 보일지 (수입, 받은 이체) */
fun isInflow(tx: Tx, viewLedgerId: Int?): Boolean = tx.isIncome || transferDirection(tx, viewLedgerId) == TransferDirection.IN

/** 목록에 보일 금액: 들어온 돈은 +, 지출은 그대로(환불은 −) */
fun txAmountText(tx: Tx, viewLedgerId: Int?): String =
    if (isInflow(tx, viewLedgerId)) signedWon(tx.amount) else "${amountText(tx.amount)}원"

/** 장부 · 항목(또는 이체) · 결제수단 · 할부 */
fun txDetail(tx: Tx, ref: RefData?, viewLedgerId: Int?, showLedger: Boolean): String = listOfNotNull(
    if (showLedger && !tx.isTransfer) ref?.ledger(tx.ledgerId)?.name else null,
    transferLabel(tx, ref, viewLedgerId) ?: ref?.category(tx.categoryId)?.name ?: "미분류",
    ref?.paymentMethod(tx.paymentMethodId)?.name,
    tx.installmentMonths.takeIf { it > 1 }?.let { "${it}개월 할부" },
    tx.member,
).joinToString(" · ")

/** 혜택 합계 "3,200원 · 1,200P · 35마일" (0인 종류는 빼고, 모두 0이면 "0원") */
fun benefitTotalsText(r: CardReport): String = listOfNotNull(
    r.discountTotal.takeIf { it != 0L }?.let { "${amountText(it)}원" },
    r.pointTotal.takeIf { it != 0L }?.let { "${amountText(it)}P" },
    r.mileageTotal.takeIf { it != 0L }?.let { "${amountText(it)}마일" },
).joinToString(" · ").ifEmpty { "0원" }

val SOURCE_LABEL = mapOf("MANUAL" to "직접 입력", "IMPORT" to "가져오기", "API" to "자동 입력", "RECURRING" to "반복 규칙")

val PAYMENT_KIND_LABEL = mapOf(
    "CREDIT_CARD" to "신용카드",
    "CHECK_CARD" to "체크카드",
    "CASH" to "현금",
    "BANK_TRANSFER" to "계좌이체",
    "OTHER" to "기타",
)
