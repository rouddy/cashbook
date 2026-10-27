"""특정 시트의 지정 셀 수식을 전체 출력. usage: python -I cell_formulas.py <xlsx> <sheet> <cell>..."""
import sys
import openpyxl

wb = openpyxl.load_workbook(sys.argv[1], read_only=False, data_only=False)
ws = wb[sys.argv[2]]
for c in sys.argv[3:]:
    print(c, "=>", ws[c].value)
