# 가계부 (cashbook)

구글 스프레드시트로 쓰던 가계부(월급사용내역·용돈·카드별 시트)를 데이터베이스로 옮겨,
지출/지출 예정, 카드 실적·혜택, 카드 결제 예정 금액, 은행 잔고 기반 여유자금을 한 화면에서 보는 서비스입니다.

- **백엔드**: Rust · Rocket 0.5 · async-graphql 7 (GraphQL API) · SeaORM 2 · SQLite
- **프론트엔드**: React 19 · TypeScript · Vite (Node로 빌드, 빌드 결과는 Rocket이 서빙)
- **가져오기 도구**: Python + openpyxl (스프레드시트 xlsx → 정규화 JSON)

| 문서 | 내용 |
|---|---|
| [server/README.md](server/README.md) | 서버 코드 구조, GraphQL 리졸버 위치, 데이터베이스 ERD |
| [web/README.md](web/README.md) | 프론트엔드 코드 구조, 화면 명세 |

## 화면

| 화면 | 내용 |
|---|---|
| 대시보드 `/` | 이번 달 소비 vs 평균·예산, 여유자금, 다가오는 카드 결제, 예정 지출, 월별 추이 |
| 가계부 `/transactions` | 스프레드시트처럼 보고 바로 고치는 월별 내역과 시트식 요약 |
| 결제 입력 `/entry` | 휴대폰용 입력 폼, 카드 승인 문자 붙여넣기 |
| 카드 `/cards` | 카드 목록·추가·삭제, 다음 결제일·금액, 이번 달 실적 |
| 카드 상세 `/cards/:id` | 카드 정보·정산 일정, 청구서별 결제 예정 금액, 실적·혜택, 카드 거래 |
| 카드별 내역 `/card-transactions` | 카드별 월 이용 내역과 혜택액 (이용월/결제월 기준) |
| 은행 잔고 `/accounts` | 계좌, 잔고 기록과 이력, 여유자금 포함 여부 |
| 지출 계획 `/plan` | 카테고리 예산, 월 수입·배분, 반복 지출 규칙 |
| 설정 `/settings` | 장부, 카드 외 결제수단, 자동 입력 API 토큰과 사용법 |

화면별 구성, URL 파라미터, 쓰는 API는 [web/README.md](web/README.md#화면-명세)에 있습니다.

## 빠른 시작

필요: Rust 1.85+ (edition 2024), Node 20+, (데이터 가져오기 시) Python 3.10+

```bash
# 1) 웹 빌드
cd web
npm install
npm run build          # → web/dist

# 2) 서버 실행 (server 디렉터리에서 실행해야 기본 경로가 맞습니다)
cd ../server
cargo run              # http://127.0.0.1:8000  (GraphQL 탐색기: /api/graphiql)
```

개발 중에는 서버(`cargo run`)를 띄운 채로 `cd web && npm run dev` → http://127.0.0.1:5173 (Vite가 `/api`를 8000으로 넘김. 다른 서버로 넘기려면 `CASHBOOK_API=http://127.0.0.1:8001 npm run dev`).
웹 타입 검사·린트: `cd web && npx tsc -b && npm run lint`.

### 스프레드시트 데이터 가져오기

스프레드시트를 xlsx로 내려받아 `import/raw/<이름>/` 아래에 둔 뒤:

```bash
python3 -m venv tools/.venv && tools/.venv/bin/pip install -r tools/requirements.txt
tools/.venv/bin/python -I tools/export_sheets.py \
  --household "import/raw/salary2020/월급사용내역 2020.xlsx" \
  --allowance "import/raw/allowance2020/용돈 2020.xlsx" \
  --mgs "import/raw/mg_s/MG+S.xlsx" \
  --shinhan "import/raw/shinhan_altteul/알뜰교통 신한카드.xlsx" \
  --jeongseok "import/raw/card_jeongseok/카드의정석.xlsx" \
  --kb "import/raw/kb_card/KB카드종합.xlsx" \
  --out import/normalized.json

cd server
cargo run -- import ../import/normalized.json            # 빈 DB에 넣기
cargo run -- import ../import/normalized.json --replace  # 기존 데이터(토큰 제외)를 지우고 다시 넣기 ⚠️
```

`--replace`는 앱에서 입력한 거래·잔고·카드 정산 일정까지 모두 지웁니다. 처음 한 번만 쓰세요.

가져오기 규칙 (`tools/export_sheets.py`):

- 월 시트 이름 `YYM`/`YYMM`(예: `255`=2025-05, `2610`=2026-10)을 시트 월로 해석합니다.
- 열: Date / Expenditure(금액) / Item(항목) / Type(결제수단) / Detail(내용) / 송화(`O` → 구성원 "송화").
- 날짜가 시트 월에서 크게 벗어나면 연도 오타 → 월 오타 순으로 보정하고, 그래도 아니면(다른 달 환불을 원 구매월 시트에 적은 경우) 시트 월로 넣습니다. 원래 날짜는 메모에 남깁니다.
- 날짜 없이 `(25)아파트관리비`처럼 적힌 행은 **예정 거래**입니다. 실제 내역이 없는 달(2609·2610)의 행은 예정으로, 실제 내역이 있는 달의 행은 그날 확정으로 넣고, 최신 템플릿으로 **반복 규칙**을 만듭니다.
- `계획` 시트 D열 → 카테고리 월 예산, 월 시트의 월급/용돈/적금 블록 → 지출계획 항목(송화 용돈은 구성원 용돈).
- 카드 시트(MG+S·알뜰교통·카드의정석·KB)의 혜택 카테고리는 (카드, 날짜, 금액)으로 가계부 거래와 맞춰 혜택 태그로 넣고, 카드별 혜택률·한도·전월실적 구간은 각 시트의 계산식을 옮겼습니다.
- 최근 1년 사용이 없는 결제수단은 보관 처리합니다. **카드 정산 일정(결제일·이용기간)은 시트에 없으므로 카드 상세에서 직접 입력하세요.**

검증: `tools/verify_totals.py`로 모든 월 시트의 `총 사용 금액`과 변환 결과의 시트별 합계를 비교합니다(2020-01 ~ 2026-10 가계부 82개·용돈 82개 시트 모두 일치 확인).
앱에서는 거래일 기준으로 월을 나누므로, 시트 월과 다른 날짜로 적힌 행(예: 2608 시트의 7월 교통비)만큼 화면의 월 합계는 시트와 다를 수 있습니다.

```bash
tools/.venv/bin/python -I tools/verify_totals.py "import/raw/salary2020/월급사용내역 2020.xlsx" import/normalized.json
```

분석용 보조 스크립트: `inspect_xlsx.py`(시트 내용/수식 보기), `survey_sheets.py`(시트별 헤더·행 수), `distinct_values.py`(항목·결제수단 분포), `date_outliers.py`(날짜 이상치), `card_tags.py`(카드 시트 혜택 태그), `cell_formulas.py`(셀 수식).

옮기지 않은 것: `자산` 시트의 적립금 누적(경조사비·저축·여행자금·송화돈), `적금` 시트의 해약된 상품, `신용카드분석`/`대한항공 포인트` 비교표(보유카드의 카드사·유효기간·연회비만 카드 정보에 반영), 월 시트의 누적 `남는 금액`.

### 운영 (릴리스 빌드)

```bash
cd web && npm ci && npm run build
cd ../server && cargo build --release
# server 디렉터리에서 실행 (data/cashbook.db, ../web/dist 기준)
CASHBOOK_PASSWORD='긴-비밀번호' ROCKET_ADDRESS=0.0.0.0 ./target/release/cashbook
```

- 세션 쿠키 키는 `data/secret.key`에 자동으로 만들어 두고 재시작해도 그대로 씁니다(로그인 유지).
- DB 백업: `sqlite3 data/cashbook.db ".backup backup-$(date +%F).db"` (서버 실행 중에도 안전).
- 반복 규칙의 예정 거래는 서버가 시작할 때와 매시간 채웁니다. 수동 실행: `./target/release/cashbook generate-planned`.

## 설정 (환경 변수)

`server/.env`에 둘 수 있습니다 (`server/.env.example` 참고).

| 변수 | 기본값 | 설명 |
|---|---|---|
| `DATABASE_URL` | `sqlite://data/cashbook.db?mode=rwc` | SQLite 파일 (실행 위치 기준) |
| `CASHBOOK_PASSWORD` | (없음) | 설정하면 웹 로그인과 API 토큰 인증을 요구 |
| `CASHBOOK_TZ` | `Asia/Seoul` | "오늘" 기준 시간대 |
| `CASHBOOK_PLAN_MONTHS` | `2` | 반복 규칙으로 이번 달 + N개월 예정 거래 생성 |
| `CASHBOOK_WEB_DIR` | `../web/dist` 또는 `web/dist` | React 빌드 위치 |
| `CASHBOOK_SECRET_FILE` | `data/secret.key` | 세션 쿠키 키 (없으면 생성, `ROCKET_SECRET_KEY`가 우선) |
| `CASHBOOK_SECURE_COOKIE` | (끔) | HTTPS 프록시 뒤에서 `1` |
| `CASHBOOK_ALLOWED_ORIGINS` | (없음) | 브라우저 요청의 Origin이 Host와 다를 때 허용할 출처 (프록시가 Host를 바꾸는 경우, 예: `https://cash.example.com`) |
| `CASHBOOK_ALLOWED_HOSTS` | (없음) | 비밀번호 없이 실행할 때 localhost 외에 허용할 Host 이름 |
| `CASHBOOK_ALLOW_INSECURE` | (끔) | 비밀번호 없이 외부 주소로 여는 것을 허용 (권장하지 않음) |
| `ROCKET_ADDRESS`, `ROCKET_PORT` | `127.0.0.1`, `8000` | `server/Rocket.toml` |

**외부(휴대폰 자동 입력 등)에서 접속하려면** `CASHBOOK_PASSWORD`를 설정하고 `ROCKET_ADDRESS=0.0.0.0`으로 띄운 뒤, HTTPS 리버스 프록시(nginx·Caddy 등) 뒤에 두고 `CASHBOOK_SECURE_COOKIE=1`을 켜세요. 비밀번호 없이 외부 주소로 띄우면 서버가 시작을 거부합니다.

보안 동작:
- 로그인 세션 쿠키는 암호화·HttpOnly·SameSite=Strict, 30일 유지(재시작해도 유지). 비밀번호를 바꾸면 기존 세션은 무효가 됩니다.
- 보호된 API(`/api/graphql`, `/api/ingest`)는 브라우저가 다른 사이트에서 보낸 요청(Origin이 Host와 다름)을 403으로 거부합니다. 자동화 앱처럼 Origin을 보내지 않는 요청은 영향이 없습니다. 프록시가 Host를 바꾸면 `X-Forwarded-Host`를 넘기거나 `CASHBOOK_ALLOWED_ORIGINS`를 설정하세요.
- 비밀번호 없이(로컬) 실행할 때는 `localhost`/`127.0.0.1` 주소로 온 요청만 받습니다(DNS 리바인딩 방지).
- 로그인 실패는 IP당 10분에 10번, 전체 10분에 100번까지.
- API 토큰은 SHA-256 해시만 저장하고, 기본 권한은 **자동 입력 전용**입니다(`/api/ingest`만). GraphQL 전체를 스크립트로 쓰려면 설정에서 "전체 API" 토큰을 따로 발급하세요.

## 핵심 계산 규칙

### 소비·월잔액 (시트와 같은 기준)
- 소비 = 지출 카테고리 합계. `합계 제외` 카테고리(경조사비·여행 — 별도 적립금에서 쓰는 항목)는 따로 집계.
- 월잔액 = 계획 수입(월급) + 수입 카테고리(기타수익) − 소비 − 배분(재홍 용돈·경조사비·저축·여행적금).
- 구성원 용돈(송화 용돈)은 월잔액에서 빼지 않고, 그 구성원 표시 지출과 비교해 잔액을 보여줍니다.
- 평균은 이전 N개월 중 확정 거래가 있는 달만으로 계산합니다.

### 지출 예정
- 거래 상태: `확정(CONFIRMED)` / `예정(PLANNED)` / `건너뜀(SKIPPED)`.
- 반복 규칙은 이번 달 + `CASHBOOK_PLAN_MONTHS`개월치 예정 거래를 만들고(서버 시작 시·매시간), 규칙을 고치면 이번 달 이후 예정분을 다시 만듭니다.
- 반복 규칙이 이번 달 이후에 만든 항목을 지우면(확정·예정 모두) 그 달은 `건너뜀`으로 남아 다시 생기지 않습니다. 건너뛴 항목은 `복원`으로 되살립니다.
- 결제수단을 보관(거래가 있는 카드 삭제)하면 그 결제수단의 반복 규칙을 끄고 이번 달 이후 예정 항목을 지웁니다.
- 날짜가 지났는데 확정되지 않은 예정 항목은 대시보드·가계부에 `! 미확정`으로 표시됩니다.

### 카드 정산 일정과 결제 예정 금액
- 카드마다 **결제일**, **이용기간 마감월**(결제월 기준 당월/전월/전전월), **마감일**(말일 가능)을 입력합니다.
  예: 25일 결제·당월 11일 마감 → 전월 12일 ~ 당월 11일 이용분이 25일에 결제.
  결제일만 고르고 "결제일로 자동 추정"을 누르면 카드사 일반 기준(결제일 약 14일 전 마감)으로 채웁니다. 실제 명세서와 맞는지 확인하세요.
- 결제일이 주말이면 다음 월요일 출금으로 계산합니다(공휴일은 반영하지 않음).
- 할부는 회차별로 나눠 각 청구서에 넣습니다(나머지는 첫 회차).

### 여유자금
```
현재 여유자금 = 은행 잔고(여유자금 포함 계좌) − 잔고 기준일 이후 결제될 카드대금(확정 이용분)
                − 잔고 기준일 이후의 계좌 출금(체크카드·계좌이체 등, 연결 계좌가 있는 결제수단)
예정 반영 후   = 현재 여유자금 − 이번 달까지의 예정 지출(카드 예정 포함) + 예정 수입
```
카드에 결제 계좌를 연결하면 그 계좌의 잔고 기준일을, 아니면 포함된 계좌 중 가장 최근 기준일을 씁니다.
결제 계좌가 여유자금에서 제외된 계좌(적금 등)인 카드의 대금과 그 계좌의 예정 출금은 빼지 않습니다(목록에는 "여유자금 제외 계좌에서 출금"으로 표시).
정산 일정이 없는 신용카드는 카드대금에서 빠지고 경고로 표시됩니다.

### 카드 실적·혜택 (카드 소비 계획)
- 실적 = 해당 달(이용일 기준) 카드 이용금액 합계. `실적 제외` 표시 거래와 "혜택받은 금액은 실적 제외" 혜택이 붙은 거래는 뺍니다.
- 이번 달 통합 혜택 한도 = 지난달 실적이 도달한 구간의 한도. 다음 구간까지 남은 금액도 보여줍니다.
- 건별 혜택 = 이용금액 × 혜택률 (건당 최소 결제금액 미만이면 0, 건당 최대로 제한) → 항목별 월 한도 → 통합 한도를 날짜순으로 누적 적용. 환불은 그 달에 받은 혜택까지만 되돌립니다.
- 한도 방식: `통합한도 차감`이면 전월실적 구간 한도에서 빠집니다. 끄고 항목 월한도를 넣으면 별도 한도, 둘 다 없으면 한도 없이 받습니다(예: 카드의정석 etc 0.8%). 카드 화면의 한도 사용액은 통합 한도에서 차감된 혜택만 셉니다.
- 거래 하나에 고른 혜택은 모두 더합니다(백화점 1% + 간편결제 3% = 4%). `중복 적용`이 아닌 혜택은 거래당 하나만 고를 수 있고, 서버도 둘 이상이면 거절합니다.
- 혜택 종류: 할인(원) / 포인트 적립(P, 1P = 1원) / 마일리지 적립(마일). 마일리지 혜택률은 "N원당 1마일"로 입력하고, 건당 최대·항목 월한도도 그 단위로 적습니다. 혜택 합계는 종류별로 따로 보여줍니다.
- 거래에 혜택 항목을 지정해야 계산됩니다(가계부/카드별 내역의 "혜택" 칸, 결제 입력 폼). 자동 입력은 같은 가맹점의 과거 혜택 지정을 따라갑니다.

## 자동 입력 API

여러 플랫폼에서 결제 내역을 자동으로 넣기 위한 REST 엔드포인트입니다. 인증이 켜져 있으면 설정 화면에서 발급한 토큰(기본: 자동 입력 전용)을 `Authorization: Bearer <토큰>`(또는 `X-Api-Key`)으로 보냅니다.

```bash
# 카드 승인 문자 원문 그대로 (금액·가맹점·일시·할부·취소·카드사·끝자리를 서버가 해석)
curl -X POST https://<서버>/api/ingest -H 'Authorization: Bearer cb_...' \
  -H 'Content-Type: text/plain' \
  --data-binary '[Web발신] 신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원'

# JSON (한 건 또는 배열)
curl -X POST https://<서버>/api/ingest -H 'Authorization: Bearer cb_...' \
  -H 'Content-Type: application/json' \
  -d '{"amount": 12500, "merchant": "스타벅스", "card": "MG+S", "date": "2026-10-23", "externalId": "pay-001"}'
```

| 필드 | 설명 |
|---|---|
| `amount` | 금액(원). 없으면 `text`에서 찾음 |
| `merchant` / `description` | 내용(가맹점) |
| `card` / `paymentMethod` | 결제수단 이름·별칭 또는 카드번호 끝 4자리 |
| `date` | `YYYY-MM-DD` (없으면 문자 날짜 → 오늘) |
| `category`, `ledger`, `member`, `memo` | 이름으로 지정 (없으면 과거 내역으로 추천, 장부는 기본 장부) |
| `installmentMonths` | 할부 개월 |
| `cancel` | `true`면 취소/환불 (음수 저장) |
| `text` | 승인 문자/알림 원문 (명시 필드가 우선) |
| `externalId` | 중복 방지 키 (없으면 원문 해시) |
| `planned` | `true`면 예정 거래로 저장 |

처리 순서: 원문 해석 → 중복 확인 → 결제수단 찾기(이름·별칭 → 끝 4자리 → 원문의 카드 이름 → 카드사가 하나뿐일 때) →
장부/카테고리 추천 → 같은 결제수단 ±7일 예정 항목과 금액이나 내용(또는 반복 규칙 매칭 키워드)이 맞으면 그 항목을 확정 → 아니면 새로 저장.
같은 카드사 카드가 여러 장이면 카드번호 끝 4자리를 등록해야 문자만으로 구분됩니다.
자동 입력된 거래에는 받은 원문을 `raw_text`로 남깁니다: `text`(또는 text/plain 본문)는 받은 그대로, `text` 없이 필드만 보내면 받은 필드를 JSON으로(`merchant`·`card`는 `description`·`paymentMethod`로). 가계부 그리드의 "자동" 표시에 마우스를 올리면 보입니다.

응답 예: `{"ok": true, "created": true, "duplicate": false, "matchedPlanned": false, "warnings": [], "transaction": {"id": 8612, "amount": 12500, "category": "외식", "paymentMethod": "MG+S", ...}}`

플랫폼별 연결: 아이폰은 단축어 자동화(메시지 수신 → "URL의 콘텐츠 가져오기"로 POST), 안드로이드는 문자 전달 앱·MacroDroid·Tasker의 HTTP 요청으로 같은 엔드포인트를 호출합니다 (설정 화면에 단계별 안내).

GraphQL로도 같은 기능을 씁니다 (로그인 세션 또는 "전체 API" 토큰): `mutation { ingestTransaction(input: {text: "..."}) { created duplicate transaction { id } } }`

## GraphQL API

엔드포인트 `POST /api/graphql` (`Content-Type: application/json`, 로그인 세션 또는 "전체 API" 토큰).
전체 스키마는 `/api/graphiql`에서 확인하세요. Query·Mutation 목록과 리졸버 위치는 [server/README.md](server/README.md#graphql-요청)에 있습니다.

그 밖의 REST: `GET /api/session`, `POST /api/login`, `POST /api/logout`, `GET /api/health`.

## 데이터베이스

SQLite(`server/data/cashbook.db`)이며 마이그레이션은 서버 시작 시 자동 적용됩니다.
테이블 관계(ERD), 외래 키·제약, enum 값, 금액 부호 규칙은 [server/README.md](server/README.md#데이터베이스)에 있습니다.

## 테스트

```bash
cd server && cargo test        # 정산 일정·할부·혜택·여유자금·월 요약·문자 해석·자동 입력·반복 규칙·인증 (40개)

# 브라우저 E2E (시스템 Chrome + playwright-core). 반드시 테스트용 DB 사본으로 띄운 서버에 대해 실행
sqlite3 server/data/cashbook.db ".backup /tmp/cashbook-e2e.db"
(cd server && DATABASE_URL="sqlite:///tmp/cashbook-e2e.db?mode=rwc" CASHBOOK_PASSWORD=e2e-pass \
   CASHBOOK_SECRET_FILE=/tmp/cashbook-e2e.key ROCKET_PORT=8001 cargo run) &
cd web && npm run e2e          # 로그인, 셀 편집·키보드, 입력, 카드 추가/삭제, 잔고, 반복 규칙, 토큰 권한·교차 사이트 차단 (19단계)
```

E2E 환경 변수: `E2E_BASE_URL`(기본 `http://127.0.0.1:8001`), `E2E_PASSWORD`(기본 `e2e-pass`), `CHROME_PATH`(기본 macOS Google Chrome 경로).

## 알려진 제한

- 카드 결제일 계산은 주말만 다음 영업일로 미루고 공휴일은 고려하지 않습니다.
- 실적은 이용일 기준 달력월로 계산합니다. 카드사가 매입일·결제일 기준으로 실적을 잡거나 특정 업종을 자동 제외하는 경우 `실적 제외` 표시로 맞춰야 합니다.
- 탄탄대로 Biz처럼 실적 구간에 따라 항목별 한도가 바뀌는 카드는 현재 구간의 항목 한도로 입력되어 있습니다.
- 승인 문자 해석은 대표적인 카드사 형식을 기준으로 한 휴리스틱입니다. 못 찾은 값은 경고와 함께 비워 두며 JSON 필드로 보완할 수 있습니다.
