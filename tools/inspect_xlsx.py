"""xlsx 구조 확인용: 시트 목록, 크기, 앞쪽 행 값(필요하면 수식)을 출력한다.

usage: python -I inspect_xlsx.py <xlsx> [--rows N] [--formulas] [--sheet NAME] [--start R]
"""
import argparse
import openpyxl


def fmt(v):
    if v is None:
        return ""
    s = str(v)
    return s if len(s) <= 40 else s[:37] + "..."


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("--rows", type=int, default=15)
    ap.add_argument("--cols", type=int, default=30)
    ap.add_argument("--start", type=int, default=1)
    ap.add_argument("--formulas", action="store_true")
    ap.add_argument("--sheet", action="append")
    ap.add_argument("--list", action="store_true", help="시트 목록과 크기만")
    args = ap.parse_args()

    wb = openpyxl.load_workbook(args.path, read_only=True, data_only=not args.formulas)
    for ws in wb.worksheets:
        if args.sheet and ws.title not in args.sheet:
            continue
        print(f"=== [{ws.title}] max_row={getattr(ws, 'max_row', None)} max_col={getattr(ws, 'max_column', None)}")
        if args.list:
            continue
        end = args.start + args.rows - 1
        for r_idx, row in enumerate(
            ws.iter_rows(min_row=args.start, max_row=end, max_col=args.cols, values_only=True),
            start=args.start,
        ):
            cells = [fmt(v) for v in row]
            while cells and cells[-1] == "":
                cells.pop()
            if cells:
                print(f"{r_idx:>4}: " + " | ".join(cells))


if __name__ == "__main__":
    main()
