"""카드 시트의 (결제수단, 혜택카테고리, 간편결제 플래그) 분포와 최근 사용월을 집계한다.
usage: python -I card_tags.py <xlsx> <type_col_idx> <tag_col_idx> [<flag_col_idx>]
"""
import collections, re, sys, datetime as dt
import openpyxl
wb = openpyxl.load_workbook(sys.argv[1], read_only=True, data_only=True)
tc, gc = int(sys.argv[2]), int(sys.argv[3])
fc = int(sys.argv[4]) if len(sys.argv) > 4 else None
cnt = collections.Counter(); last = {}
for ws in wb.worksheets:
    if not re.match(r"^\d{3,4}$", ws.title):
        continue
    for row in ws.iter_rows(min_row=2, max_col=10, values_only=True):
        if not isinstance(row[1], (int, float)):
            continue
        key = (row[tc], row[gc], row[fc] if fc is not None else None)
        cnt[key] += 1
        if isinstance(row[0], (dt.date, dt.datetime)):
            last[key] = max(last.get(key, row[0]), row[0])
for k, v in sorted(cnt.items(), key=lambda kv: -kv[1]):
    l = last.get(k)
    print(v, k, l.strftime("%Y-%m") if l else "")
