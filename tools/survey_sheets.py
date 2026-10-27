"""모든 월별 시트의 헤더(1행)와 날짜 있는 행 수/날짜 범위를 요약한다.

usage: python -I survey_sheets.py <xlsx>
"""
import datetime as dt
import sys

import openpyxl


def main():
    wb = openpyxl.load_workbook(sys.argv[1], read_only=True, data_only=True)
    headers = {}
    for ws in wb.worksheets:
        rows = list(ws.iter_rows(min_row=1, max_col=8, values_only=True))
        if not rows:
            print(f"[{ws.title}] (empty)")
            continue
        header = tuple("" if v is None else str(v) for v in rows[0][:7])
        dated = [r for r in rows[1:] if isinstance(r[0], (dt.date, dt.datetime))]
        undated = [r for r in rows[1:] if r[0] is None and isinstance(r[1], (int, float)) and r[4]]
        other_a = [r[0] for r in rows[1:] if r[0] is not None and not isinstance(r[0], (dt.date, dt.datetime))]
        rng = ""
        if dated:
            ds = sorted(r[0] for r in dated)
            rng = f"{ds[0]:%Y-%m-%d}~{ds[-1]:%Y-%m-%d}"
        headers.setdefault(header, []).append(ws.title)
        print(f"[{ws.title}] dated={len(dated)} undated_amount_rows={len(undated)} range={rng} nondate_A={other_a[:3]}")
    print("\n== header variants ==")
    for h, names in headers.items():
        print(h, "=>", names[:6], "..." if len(names) > 6 else "", len(names))


if __name__ == "__main__":
    main()
