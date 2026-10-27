"""월별 시트에서 시트 월과 동떨어진 날짜 행, 날짜 없는 금액 행을 출력한다.

usage: python -I date_outliers.py <xlsx>
"""
import datetime as dt
import re
import sys

import openpyxl

SHEET_RE = re.compile(r"^(\d{2})(\d{1,2})$")


def sheet_month(title):
    m = SHEET_RE.match(title)
    if not m:
        return None
    return 2000 + int(m.group(1)), int(m.group(2))


def main():
    wb = openpyxl.load_workbook(sys.argv[1], read_only=True, data_only=True)
    for ws in wb.worksheets:
        ym = sheet_month(ws.title)
        if not ym:
            continue
        y, m = ym
        start = dt.date(y, m, 1)
        end = (dt.date(y + (m == 12), m % 12 + 1, 1) - dt.timedelta(days=1))
        for idx, row in enumerate(ws.iter_rows(min_row=2, max_col=6, values_only=True), start=2):
            d, amt, item, typ, detail = row[:5]
            if not isinstance(amt, (int, float)):
                continue
            if isinstance(d, (dt.date, dt.datetime)):
                dd = d.date() if isinstance(d, dt.datetime) else d
                if dd < start - dt.timedelta(days=31) or dd > end + dt.timedelta(days=10):
                    print(f"[{ws.title}] r{idx} OUTLIER {dd} {amt} {item} {typ} {detail}")
            elif d is None and detail:
                print(f"[{ws.title}] r{idx} UNDATED {amt} {item} {typ} {detail}")
            elif d is not None:
                print(f"[{ws.title}] r{idx} BADDATE {d!r} {amt} {item} {typ} {detail}")


if __name__ == "__main__":
    main()
