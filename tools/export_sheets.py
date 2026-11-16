"""구글 스프레드시트(xlsx로 내보낸 파일)를 cashbook 서버가 가져올 수 있는 정규화 JSON으로 변환한다.

usage:
  python -I tools/export_sheets.py \
    --household "import/raw/salary2020/월급사용내역 2020.xlsx" \
    --allowance "import/raw/allowance2020/용돈 2020.xlsx" \
    --mgs "import/raw/mg_s/MG+S.xlsx" \
    --shinhan "import/raw/shinhan_altteul/알뜰교통 신한카드.xlsx" \
    --jeongseok "import/raw/card_jeongseok/카드의정석.xlsx" \
    --kb "import/raw/kb_card/KB카드종합.xlsx" \
    --out import/normalized.json

생성된 JSON은 `cargo run -- import import/normalized.json`으로 DB에 넣는다.
"""
import argparse
import calendar
import collections
import datetime as dt
import json
import re
import sys

import openpyxl

SHEET_RE = re.compile(r"^(\d{2})(\d{1,2})$")
PLANNED_PREFIX_RE = re.compile(r"^\((\d{1,2})\)\s*")
# 시트 월 기준으로 이 범위를 벗어난 날짜는 오타/다른 달 귀속으로 보고 보정한다.
WINDOW_BEFORE = dt.timedelta(days=45)
WINDOW_AFTER = dt.timedelta(days=20)
# 마지막 사용일이 이 날짜 이전인 결제수단은 보관(archived) 처리한다.
ACTIVE_SINCE = dt.date(2025, 10, 23)

HOUSEHOLD = "household"
ALLOWANCE = "allowance"

LEDGERS = [
    {"key": HOUSEHOLD, "name": "가계부", "description": "월급사용내역 (생활비)", "isDefault": True},
    {"key": ALLOWANCE, "name": "용돈", "description": "재홍 용돈", "isDefault": False},
]

# 시트에서 쓰던 결제수단 표기 → 대표 이름
PAYMENT_ALIASES = {
    "알뜰교통 신한카드": "알뜰교통신한",
}

# 결제수단 기본 정보. 시트 '보유카드'와 카드별 시트에서 확인되는 것만 채우고 나머지는 비워둔다.
PAYMENT_INFO = {
    "현금": {"kind": "CASH"},
    "송화": {"kind": "OTHER", "memo": "송화가 직접 결제"},
    "탄탄대로biz": {"issuer": "KB국민", "productName": "탄탄대로 Biz", "expiresOn": "2031-04", "annualFee": 15000},
    "카드의정석": {"issuer": "우리", "productName": "카드의정석", "expiresOn": "2027-01", "annualFee": 10000},
    "알뜰교통신한": {"issuer": "신한", "productName": "알뜰교통 신한카드", "expiresOn": "2028-07", "annualFee": 12000,
                 "aliases": ["알뜰교통 신한카드", "알뜰교통카드"]},
    "스카이패스": {"issuer": "KB국민", "productName": "스카이패스 티타늄", "expiresOn": "2031-09", "annualFee": 45000},
    "CJ카드": {"issuer": "KB국민", "productName": "CJ카드", "expiresOn": "2032-05", "annualFee": 1000},
    "현대카드": {"issuer": "현대", "productName": "대한항공카드 030", "expiresOn": "2029-05", "annualFee": 30000},
    "MG+S": {"issuer": "하나", "productName": "MG+ S 하나카드"},
    "삼성카드": {"issuer": "삼성"},
    "달달카드": {"issuer": "삼성", "productName": "삼성 달달카드"},
    "이마트kb": {"issuer": "KB국민"},
    "가온올림": {"issuer": "KB국민"},
    "아시아나올림": {"issuer": "KB국민"},
    "국민": {"issuer": "KB국민"},
    "1Q데일리": {"issuer": "하나"},
    "웰컴하나카드": {"issuer": "하나"},
    "원더카드": {"issuer": "하나"},
    "원더프리+": {"issuer": "하나"},
    "딥드림": {"issuer": "신한"},
    "제로": {"issuer": "현대", "productName": "현대카드 ZERO"},
}

# 카드별 혜택/전월실적 구간. 각 카드 시트의 최신 계산식(2608 시트)을 옮긴 것.
CARD_BENEFITS = {
    "MG+S": {
        "monthlyTarget": 300000,
        "benefits": [
            {"name": "간편결제", "rate": 0.1, "minAmount": 10000},
            {"name": "디지털멤버십", "rate": 0.5, "minAmount": 10000},
            {"name": "영상스트리밍", "rate": 0.5, "minAmount": 10000},
        ],
        "tiers": [
            {"minSpending": 300000, "benefitLimit": 15000},
            {"minSpending": 600000, "benefitLimit": 30000},
            {"minSpending": 1000000, "benefitLimit": 60000},
        ],
    },
    "알뜰교통신한": {
        "monthlyTarget": 300000,
        "benefits": [
            {"name": "대중교통,택시", "rate": 0.1, "perTxMax": 10000},
            {"name": "주말마트", "rate": 0.1, "perTxMax": 5000},
            {"name": "편의점,커피,의료", "rate": 0.1, "perTxMax": 1000},
            {"name": "전기차", "rate": 0.3, "perTxMax": 20000, "monthlyCap": 20000, "sharesCardLimit": False},
        ],
        "tiers": [
            {"minSpending": 300000, "benefitLimit": 10000},
            {"minSpending": 500000, "benefitLimit": 20000},
            {"minSpending": 1000000, "benefitLimit": 30000},
        ],
    },
    "카드의정석": {
        "monthlyTarget": 300000,
        "benefits": [
            {"name": "대중교통", "rate": 0.05, "minAmount": 10000},
            {"name": "백화점/주유", "rate": 0.01, "minAmount": 10000},
            {"name": "영화/커피", "rate": 0.03, "minAmount": 10000},
            {"name": "온라인쇼핑", "rate": 0.01, "minAmount": 10000},
            {"name": "이동통신", "rate": 0.05, "minAmount": 10000},
            {"name": "전기차충전", "rate": 0.05, "minAmount": 10000},
            {"name": "할인점", "rate": 0.01, "minAmount": 10000},
            # 간편결제는 다른 항목과 함께 받는다 (예: 백화점 1% + 간편결제 3%)
            {"name": "간편결제", "rate": 0.03, "minAmount": 10000, "stackable": True},
            # etc는 통합한도에서 차감되지 않고 한도 없이 받는다
            {"name": "etc", "rate": 0.008, "minAmount": 10000, "sharesCardLimit": False},
        ],
        "tiers": [
            {"minSpending": 300000, "benefitLimit": 10000},
            {"minSpending": 600000, "benefitLimit": 20000},
            {"minSpending": 1200000, "benefitLimit": 50000},
        ],
    },
    "탄탄대로biz": {
        "monthlyTarget": 400000,
        "benefits": [
            {"name": "탄탄대로-쇼핑몰", "rate": 0.2, "monthlyCap": 15000, "excludesPerformance": True},
            {"name": "탄탄대로-운영지원", "rate": 0.2, "monthlyCap": 20000},
            {"name": "탄탄대로-통신/전기/수도", "rate": 0.2, "monthlyCap": 20000},
            {"name": "탄탄대로-주유", "rate": 0.2, "monthlyCap": 0, "excludesPerformance": True},
            {"name": "탄탄대로-마트", "rate": 0.2, "monthlyCap": 0, "excludesPerformance": True},
        ],
        "tiers": [
            {"minSpending": 400000, "benefitLimit": 55000},
        ],
    },
    "CJ카드": {"monthlyTarget": 300000, "benefits": [], "tiers": []},
    "스카이패스": {"monthlyTarget": 300000, "benefits": [], "tiers": []},
}

# 카드 시트의 옛 혜택 이름 → 현재 이름
TAG_ALIASES = {
    "알뜰교통신한": {"편의점,커피": "편의점,커피,의료"},
    "카드의정석": {"백화점": "백화점/주유", "주유": "백화점/주유"},
}

# 계획 시트 항목 이름 → 카테고리 이름
PLAN_LABEL_TO_CATEGORY = {
    "통신비": "통신료",
    "차량유지비": "차량유지",
    "교통비": "교통",
    "외식비": "외식",
}

HOUSEHOLD_CATEGORY_ORDER = ["장보기", "외식", "차량유지", "통신료", "교통", "비용", "의료비", "기타", "경조사비", "여행", "기타수익"]
ALLOWANCE_CATEGORY_ORDER = ["점심", "저녁", "음료수", "송화", "기타", "알고리고"]
CATEGORY_FLAGS = {
    (HOUSEHOLD, "비용"): {"isFixed": True},
    (HOUSEHOLD, "경조사비"): {"excludedFromSpending": True},
    (HOUSEHOLD, "여행"): {"excludedFromSpending": True},
    (HOUSEHOLD, "기타수익"): {"kind": "INCOME"},
}


def sheet_month(title):
    m = SHEET_RE.match(title.strip())
    if not m:
        return None
    month = int(m.group(2))
    if not 1 <= month <= 12:
        return None
    return 2000 + int(m.group(1)), month


def month_bounds(y, m):
    return dt.date(y, m, 1), dt.date(y, m, calendar.monthrange(y, m)[1])


def clamp_day(y, m, d):
    return dt.date(y, m, min(d, calendar.monthrange(y, m)[1]))


def to_date(v):
    if isinstance(v, dt.datetime):
        return v.date()
    if isinstance(v, dt.date):
        return v
    if isinstance(v, str):
        s = v.strip()
        for fmt in ("%Y.%m.%d", "%Y-%m-%d", "%Y/%m/%d"):
            try:
                return dt.datetime.strptime(s, fmt).date()
            except ValueError:
                pass
    return None


def norm_text(v):
    if v is None:
        return None
    s = str(v).replace("\x08", "").strip()
    return s or None


def norm_payment(v):
    s = norm_text(v)
    if s is None:
        return None
    return PAYMENT_ALIASES.get(s, s)


def correct_date(d, y, m, notes):
    """시트 월(y, m)에서 너무 먼 날짜를 보정한다. 보정 내용은 notes에 남긴다."""
    start, end = month_bounds(y, m)
    lo, hi = start - WINDOW_BEFORE, end + WINDOW_AFTER
    if lo <= d <= hi:
        return d
    # 1) 연도 오타 (예: 2512 시트의 2026-12-31)
    for yy in (y, y - 1, y + 1):
        try:
            cand = d.replace(year=yy)
        except ValueError:
            continue
        if lo <= cand <= hi:
            notes.append(f"날짜 보정(원래 {d.isoformat()})")
            return cand
    # 2) 월 오타 (예: 237 시트의 2023-03-20 → 2023-07-20)
    cand = clamp_day(y, m, d.day)
    if d.year == y:
        notes.append(f"날짜 보정(원래 {d.isoformat()})")
        return cand
    # 3) 다른 달 거래(주로 환불)를 원 구매월 시트에 적은 경우: 시트 월로 귀속
    target = end if d > end else start
    notes.append(f"원래 날짜 {d.isoformat()} ({y % 100:02d}{m:02d} 시트에 기록)")
    return target


def read_ledger(path, ledger_key, stats):
    wb = openpyxl.load_workbook(path, read_only=True, data_only=True)
    rows = []
    plan_values = {}
    budgets = {}
    for ws in wb.worksheets:
        if ws.title.strip() == "계획":
            budgets = read_budgets(ws, ledger_key)
            continue
        ym = sheet_month(ws.title)
        if not ym:
            continue
        y, m = ym
        sheet_rows = list(ws.iter_rows(min_row=1, max_col=16, values_only=True))
        if not sheet_rows:
            continue
        header = [norm_text(v) for v in sheet_rows[0]]
        has_support_col = len(header) > 6 and header[6] == "지원금"
        parsed = []
        for idx, row in enumerate(sheet_rows[1:], start=2):
            row = list(row) + [None] * (16 - len(row))
            raw_date, amount, item, typ, detail = row[0], row[1], row[2], row[3], row[4]
            if not isinstance(amount, (int, float)) or isinstance(amount, bool):
                continue
            detail = norm_text(detail)
            item = norm_text(item)
            typ = norm_payment(typ)
            if raw_date is None and not detail and not item and not typ:
                stats["skipped_amount_only"] += 1
                continue
            parsed.append((idx, raw_date, int(round(amount)), item, typ, detail, row))
        dated_count = sum(1 for p in parsed if to_date(p[1]) is not None)
        if ledger_key == HOUSEHOLD:
            plan_values[(y, m)] = read_household_plan_block(sheet_rows)
        else:
            plan_values[(y, m)] = read_allowance_plan_block(sheet_rows)
        for idx, raw_date, amount, item, typ, detail, row in parsed:
            notes = []
            status = "CONFIRMED"
            planned_day = None
            description = detail or ""
            mm = PLANNED_PREFIX_RE.match(description)
            if mm:
                planned_day = int(mm.group(1))
                description = description[mm.end():].strip()
            d = to_date(raw_date)
            recurring = None
            if d is None:
                if raw_date is not None:
                    stats["unparsable_date"] += 1
                if planned_day is not None:
                    d = clamp_day(y, m, planned_day)
                    if dated_count == 0:
                        status = "PLANNED"
                        recurring = {"day": planned_day}
                    else:
                        notes.append(f"날짜 미기재 예정 행 → {planned_day}일 확정으로 가져옴")
                        stats["undated_template_confirmed"] += 1
                else:
                    d = month_bounds(y, m)[1]
                    notes.append("날짜 미기재")
                    stats["undated_other"] += 1
            else:
                fixed = correct_date(d, y, m, notes)
                if fixed != d:
                    stats["date_corrected"] += 1
                d = fixed
            member = None
            memo_parts = list(notes)
            if ledger_key == HOUSEHOLD:
                if norm_text(row[5]) == "O":
                    member = "송화"
                if has_support_col and norm_text(row[6]) == "O":
                    memo_parts.append("지원금")
            rows.append({
                "ledger": ledger_key,
                "date": d.isoformat(),
                "rawDate": to_date(raw_date).isoformat() if to_date(raw_date) else None,
                "amount": amount,
                "category": item,
                "paymentMethod": typ,
                "description": description,
                "member": member,
                "memo": " / ".join(memo_parts) or None,
                "status": status,
                "sheet": ws.title,
                "row": idx,
                "recurring": recurring,
            })
            stats[f"{ledger_key}_{status.lower()}"] += 1
    return rows, plan_values, budgets


def read_household_plan_block(sheet_rows):
    """H/I 열의 '월급, 재홍 용돈, ...' 블록을 읽는다."""
    out = {}
    for row in sheet_rows[1:20]:
        if len(row) > 8 and isinstance(norm_text(row[7]), str) and isinstance(row[8], (int, float)):
            out[norm_text(row[7])] = int(round(row[8]))
    return out


def read_allowance_plan_block(sheet_rows):
    header = sheet_rows[0]
    for i, v in enumerate(header[:-1]):
        if norm_text(v) == "용돈" and isinstance(header[i + 1], (int, float)):
            return {"용돈": int(round(header[i + 1]))}
    return {}


def read_budgets(ws, ledger_key):
    budgets = {}
    for row in ws.iter_rows(min_row=1, max_row=40, max_col=5, values_only=True):
        if ledger_key == HOUSEHOLD:
            label, value = norm_text(row[2]), row[3]
        else:
            label, value = norm_text(row[1]), row[2]
        # '월잔액'(가계부)/'잔액'(용돈) 아래는 누적 잔액 블록이라 예산이 아니다.
        if label in ("월잔액", "잔액"):
            break
        if label and isinstance(value, (int, float)):
            budgets.setdefault(PLAN_LABEL_TO_CATEGORY.get(label, label), int(round(value)))
    return budgets


def read_card_tags(path, default_card, stats, tag_col=5, flag_col=None, type_col=3, special=None):
    """카드 시트에서 (카드, 날짜, 금액) → 혜택 태그 목록을 읽는다."""
    wb = openpyxl.load_workbook(path, read_only=True, data_only=True)
    out = []
    for ws in wb.worksheets:
        if not sheet_month(ws.title):
            continue
        t_col, g_col, ty_col = tag_col, flag_col, type_col
        if special and ws.title in special:
            t_col, g_col, ty_col = special[ws.title]
        for row in ws.iter_rows(min_row=2, max_col=10, values_only=True):
            d = to_date(row[0])
            amount = row[1]
            if d is None or not isinstance(amount, (int, float)):
                continue
            card = norm_payment(row[ty_col]) if ty_col is not None else None
            if card is None or card not in PAYMENT_INFO and card not in CARD_BENEFITS:
                card = default_card
            if card is None:
                continue
            tags = []
            exclude_perf = False
            tag = norm_text(row[t_col]) if t_col is not None else None
            if tag and not isinstance(row[t_col], (int, float)):
                if tag == "전월실적제외":
                    exclude_perf = True
                else:
                    tags.append(TAG_ALIASES.get(card, {}).get(tag, tag))
            if g_col is not None and norm_text(row[g_col]) == "O":
                tags.append("간편결제")
            if not tags and not exclude_perf:
                continue
            out.append({"card": card, "date": d.isoformat(), "amount": int(round(amount)),
                        "description": norm_text(row[4]) or "", "tags": tags, "excludePerf": exclude_perf})
            stats["card_tag_rows"] += 1
    return out


def apply_card_tags(transactions, card_tags, stats):
    index = collections.defaultdict(list)
    for t in transactions:
        if t["status"] != "CONFIRMED" or not t["paymentMethod"]:
            continue
        key = (t["paymentMethod"], t["rawDate"] or t["date"], t["amount"])
        index[key].append(t)
    for ct in card_tags:
        cands = index.get((ct["card"], ct["date"], ct["amount"]))
        if not cands:
            stats["card_tag_unmatched"] += 1
            continue
        pick = next((c for c in cands if c["description"] == PLANNED_PREFIX_RE.sub("", ct["description"])), cands[0])
        cands.remove(pick)
        allowed = {b["name"] for b in CARD_BENEFITS.get(ct["card"], {}).get("benefits", [])}
        tags = [tg for tg in ct["tags"] if tg in allowed]
        stats["card_tag_dropped"] += len(ct["tags"]) - len(tags)
        if tags:
            pick.setdefault("benefits", [])
            for tg in tags:
                if tg not in pick["benefits"]:
                    pick["benefits"].append(tg)
        if ct["excludePerf"]:
            pick["excludeFromPerformance"] = True
        stats["card_tag_matched"] += 1


def build_categories(transactions, budgets_by_ledger):
    out = []
    for ledger_key, order in ((HOUSEHOLD, HOUSEHOLD_CATEGORY_ORDER), (ALLOWANCE, ALLOWANCE_CATEGORY_ORDER)):
        used = collections.Counter(t["category"] for t in transactions if t["ledger"] == ledger_key and t["category"])
        names = list(order) + sorted(n for n in used if n not in order)
        budgets = budgets_by_ledger.get(ledger_key, {})
        for i, name in enumerate(names):
            flags = CATEGORY_FLAGS.get((ledger_key, name), {})
            budget = budgets.get(name)
            out.append({
                "ledger": ledger_key,
                "name": name,
                "kind": flags.get("kind", "EXPENSE"),
                "monthlyBudget": budget if budget else None,
                "isFixed": flags.get("isFixed", False),
                "excludedFromSpending": flags.get("excludedFromSpending", False),
                "sortOrder": i,
            })
    return out


def build_payment_methods(transactions):
    last_used = {}
    counts = collections.Counter()
    for t in transactions:
        pm = t["paymentMethod"]
        if not pm:
            continue
        counts[pm] += 1
        d = dt.date.fromisoformat(t["date"])
        if t["status"] == "CONFIRMED":
            last_used[pm] = max(last_used.get(pm, d), d)
        else:
            last_used.setdefault(pm, d)
    out = []
    names = sorted(counts, key=lambda n: (-(last_used.get(n, dt.date.min).toordinal()), n))
    for i, name in enumerate(names):
        info = PAYMENT_INFO.get(name, {})
        last = last_used.get(name)
        pm = {
            "name": name,
            "kind": info.get("kind", "CREDIT_CARD"),
            "issuer": info.get("issuer"),
            "productName": info.get("productName"),
            "aliases": info.get("aliases", []),
            "expiresOn": info.get("expiresOn"),
            "annualFee": info.get("annualFee"),
            "memo": info.get("memo"),
            "archived": last is None or last < ACTIVE_SINCE,
            "sortOrder": i,
        }
        card = CARD_BENEFITS.get(name)
        if card:
            pm["monthlyTarget"] = card["monthlyTarget"]
            pm["benefits"] = [
                {
                    "name": b["name"],
                    "kind": b.get("kind", "DISCOUNT"),
                    "rate": b["rate"],
                    "minAmount": b.get("minAmount"),
                    "perTxMax": b.get("perTxMax"),
                    "monthlyCap": b.get("monthlyCap"),
                    "sharesCardLimit": b.get("sharesCardLimit", True),
                    "excludesPerformance": b.get("excludesPerformance", False),
                    "stackable": b.get("stackable", False),
                }
                for b in card["benefits"]
            ]
            pm["tiers"] = card["tiers"]
        out.append(pm)
    return out


def build_recurring(transactions):
    """템플릿 시트(날짜 없는 '(N)내용' 행만 있는 달)의 예정 행으로 반복 규칙을 만든다."""
    rules = {}
    for t in transactions:
        if t["status"] != "PLANNED" or not t.get("recurring"):
            continue
        key = (t["ledger"], t["description"], t["paymentMethod"])
        month = t["date"][:7]
        rule = rules.get(key)
        if rule is None:
            rule = {
                "key": f"r{len(rules) + 1}",
                "ledger": t["ledger"],
                "description": t["description"],
                "amount": t["amount"],
                "category": t["category"],
                "paymentMethod": t["paymentMethod"],
                "dayOfMonth": t["recurring"]["day"],
                "startMonth": month,
            }
            rules[key] = rule
        rule["startMonth"] = min(rule["startMonth"], month)
        # 가장 최근 달의 금액을 규칙 금액으로 쓴다.
        if month >= rule.get("_latest", ""):
            rule["_latest"] = month
            rule["amount"] = t["amount"]
        t["recurringRule"] = rule["key"]
        t["recurringMonth"] = month
    out = []
    for r in rules.values():
        r.pop("_latest", None)
        out.append(r)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--household", required=True)
    ap.add_argument("--allowance", required=True)
    ap.add_argument("--mgs")
    ap.add_argument("--shinhan")
    ap.add_argument("--jeongseok")
    ap.add_argument("--kb")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    stats = collections.Counter()
    household, h_plan, h_budgets = read_ledger(args.household, HOUSEHOLD, stats)
    allowance, a_plan, a_budgets = read_ledger(args.allowance, ALLOWANCE, stats)
    transactions = household + allowance

    card_tags = []
    if args.mgs:
        card_tags += read_card_tags(args.mgs, "MG+S", stats)
    if args.shinhan:
        # 238 시트만 열 배치가 다르다: D=카테고리(혜택), 결제수단 열 없음
        card_tags += read_card_tags(args.shinhan, "알뜰교통신한", stats, special={"238": (3, None, None)})
    if args.jeongseok:
        card_tags += read_card_tags(args.jeongseok, "카드의정석", stats, flag_col=6)
    if args.kb:
        card_tags += read_card_tags(args.kb, None, stats)
    apply_card_tags(transactions, card_tags, stats)

    recurring = build_recurring(transactions)
    categories = build_categories(transactions, {HOUSEHOLD: h_budgets, ALLOWANCE: a_budgets})
    payment_methods = build_payment_methods(transactions)

    # 가장 최근 실제 데이터가 있는 달의 계획 블록(월급/용돈/적립 등)을 계획 항목으로 쓴다.
    def latest_plan(plan_values, ledger_rows):
        months = sorted({(int(t["date"][:4]), int(t["date"][5:7])) for t in ledger_rows if t["status"] == "CONFIRMED"})
        for ym in reversed(months):
            if plan_values.get(ym):
                return plan_values[ym]
        return {}

    h_latest = latest_plan(h_plan, household)
    a_latest = latest_plan(a_plan, allowance)
    plan_items = []
    order = 0
    for name, amount in h_latest.items():
        if name == "월급":
            plan_items.append({"ledger": HOUSEHOLD, "kind": "INCOME", "name": name, "amount": amount, "sortOrder": order})
        elif name == "송화 용돈":
            # 송화 지출은 가계부에 '송화' 표시로 기록되므로 월잔액에서 따로 빼지 않는다 (시트 수식과 동일)
            plan_items.append({"ledger": HOUSEHOLD, "kind": "MEMBER_ALLOWANCE", "name": name, "amount": amount,
                               "member": "송화", "sortOrder": order})
        elif name in ("재홍 용돈", "경조사비", "저축", "여행적금"):
            plan_items.append({"ledger": HOUSEHOLD, "kind": "ALLOCATION", "name": name, "amount": amount, "sortOrder": order})
        else:
            continue
        order += 1
    for name, amount in a_latest.items():
        plan_items.append({"ledger": ALLOWANCE, "kind": "INCOME", "name": name, "amount": amount, "sortOrder": 0})

    accounts = [{
        "name": "청약종합저축",
        "bank": "하나",
        "kind": "SAVINGS",
        "balance": 3400000,
        "balanceDate": None,
        "includeInAvailable": False,
        "interestRate": 0.028,
        "memo": "스프레드시트 '적금' 시트에서 가져옴 (만기일 2024-08-01, 미해약)",
    }]

    for t in transactions:
        for k in ("rawDate", "recurring"):
            t.pop(k, None)

    data = {
        "version": 1,
        "generatedAt": dt.datetime.now().isoformat(timespec="seconds"),
        "ledgers": LEDGERS,
        "categories": categories,
        "paymentMethods": payment_methods,
        "accounts": accounts,
        "planItems": plan_items,
        "recurringRules": recurring,
        "members": ["송화"],
        "transactions": transactions,
    }
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=1)

    print(f"transactions={len(transactions)} categories={len(categories)} paymentMethods={len(payment_methods)} "
          f"recurring={len(recurring)} planItems={len(plan_items)}", file=sys.stderr)
    for k, v in sorted(stats.items()):
        print(f"  {k}: {v}", file=sys.stderr)


if __name__ == "__main__":
    main()
