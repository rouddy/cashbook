"""변환 JSON의 시트별 소비 합계가 각 월 시트의 '총 사용 금액'(I2)과 같은지 전수 확인한다.

usage: python -I verify_totals.py <월급사용내역.xlsx> <normalized.json>
"""
import collections
import json
import re
import sys

import openpyxl

SPEND = {"장보기", "외식", "차량유지", "통신료", "교통", "비용", "의료비", "기타"}

wb = openpyxl.load_workbook(sys.argv[1], read_only=True, data_only=True)
data = json.load(open(sys.argv[2], encoding="utf-8"))
sums = collections.defaultdict(int)
for t in data["transactions"]:
    if t["ledger"] == "household" and t["category"] in SPEND:
        sums[t["sheet"]] += t["amount"]

checked = mismatched = 0
for ws in wb.worksheets:
    if not re.match(r"^\d{3,4}$", ws.title):
        continue
    rows = list(ws.iter_rows(min_row=1, max_row=3, min_col=8, max_col=9, values_only=True))
    label, value = rows[1] if len(rows) > 1 else (None, None)
    if label != "총 사용 금액" or not isinstance(value, (int, float)):
        print(f"[{ws.title}] 총 사용 금액 셀 없음: {rows[1] if len(rows) > 1 else None}")
        continue
    checked += 1
    if round(value) != sums[ws.title]:
        mismatched += 1
        print(f"[{ws.title}] 시트 {round(value):,} vs 변환 {sums[ws.title]:,} (차이 {sums[ws.title] - round(value):,})")
print(f"확인 {checked}개 시트, 불일치 {mismatched}개")
