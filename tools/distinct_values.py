"""월별 시트(헤더 Date/Expenditure/Item/Type/Detail)의 Item/Type 값 분포와 마지막 사용일을 집계한다.

usage: python -I distinct_values.py <xlsx>
"""
import collections
import datetime as dt
import re
import sys

import openpyxl

SHEET_RE = re.compile(r"^\d{3,4}$")


def main():
    wb = openpyxl.load_workbook(sys.argv[1], read_only=True, data_only=True)
    items = collections.Counter()
    types = collections.Counter()
    type_last = {}
    item_last = {}
    col6 = collections.Counter()
    col7 = collections.Counter()
    for ws in wb.worksheets:
        if not SHEET_RE.match(ws.title):
            continue
        for row in ws.iter_rows(min_row=2, max_col=7, values_only=True):
            d, amt, item, typ, detail = row[:5]
            if amt is None or not isinstance(amt, (int, float)):
                continue
            items[item] += 1
            types[typ] += 1
            if isinstance(d, (dt.date, dt.datetime)):
                type_last[typ] = max(type_last.get(typ, d), d)
                item_last[item] = max(item_last.get(item, d), d)
            if row[5] not in (None, ""):
                col6[row[5]] += 1
            if len(row) > 6 and row[6] not in (None, ""):
                col7[row[6]] += 1
    print("== Item ==")
    for k, v in items.most_common():
        last = item_last.get(k)
        print(f"  {k!r}: {v} last={last:%Y-%m-%d}" if last else f"  {k!r}: {v}")
    print("== Type ==")
    for k, v in types.most_common():
        last = type_last.get(k)
        print(f"  {k!r}: {v} last={last:%Y-%m-%d}" if last else f"  {k!r}: {v}")
    print("== col F ==", col6.most_common(10))
    print("== col G ==", col7.most_common(10))


if __name__ == "__main__":
    main()
