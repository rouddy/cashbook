# cashbook web

가계부 프론트엔드. React 19 · TypeScript · Vite · TanStack Query · React Router.
`npm run build` 결과(`web/dist`)를 Rocket 서버가 `/`에서 서빙합니다 (없는 경로는 `index.html`로 SPA 라우팅).

이 문서는 프론트엔드 코드 구조와 화면 명세를 설명합니다. 다음 내용은 다른 문서에 있습니다.

- 설치·빌드·개발 서버·타입 검사·린트: [루트 README 빠른 시작](../README.md#빠른-시작)
- 브라우저 E2E 실행: [루트 README 테스트](../README.md#테스트)
- 화면이 쓰는 계산 규칙(소비·정산·여유자금·혜택): [루트 README 핵심 계산 규칙](../README.md#핵심-계산-규칙)
- GraphQL Query·Mutation과 리졸버 위치: [server/README.md](../server/README.md#graphql-요청)

## 디렉터리 구조

```text
web/
├── index.html
├── vite.config.ts            개발 서버 /api 프록시 (→ 127.0.0.1:8000), 빌드 출력 dist
├── tsconfig*.json            앱(src) / 빌드 설정(vite.config.ts) 분리
├── .oxlintrc.json            린트 설정
├── public/favicon.svg
├── e2e/smoke.mjs             playwright-core + 시스템 Chrome 스모크 테스트 (18단계)
└── src/
    ├── main.tsx              QueryClient · ToastProvider · BrowserRouter 구성
    ├── App.tsx               세션 확인 → 로그인 화면 또는 라우트 표
    ├── styles.css            디자인 토큰(라이트/다크), 레이아웃, 반응형 (CSS 파일 하나)
    ├── api/
    │   ├── client.ts         gql() 호출, 세션·로그인·로그아웃 REST, 401 → 전역 이벤트
    │   ├── hooks.ts          useRefData(기준 데이터 + id 맵), useGql, useMutate
    │   ├── queries.ts        GraphQL 문서 (필드 묶음, 화면별 쿼리, 뮤테이션)
    │   └── types.ts          응답 타입 (스키마에 맞춰 손으로 작성)
    ├── lib/
    │   ├── month.ts          "YYYY-MM" 계산, URL 상태 훅 useMonthParam / useQueryParam
    │   ├── benefits.ts       카드 혜택 종류·단위 표시, 혜택률(% / N원당 1마일), 중복 적용 선택 규칙
    │   └── format.ts         금액·날짜·비율 포맷, enum 표시 이름
    ├── components/
    │   ├── Layout.tsx        사이드바 메뉴 + 본문(Outlet)
    │   ├── TransactionGrid.tsx  스프레드시트식 거래 표 (셀 편집, 새 행 입력, 확정·건너뛰기·복원)
    │   ├── DescriptionInput.tsx 내용(가맹점) 자동완성, 고르면 과거 분류를 넘겨줌
    │   ├── CardForm.tsx      결제수단 추가/편집 (정산 일정 자동 추정, 혜택 항목·종류·중복 적용, 실적 구간)
    │   ├── BenefitChecks.tsx 거래 혜택 선택 (중복 적용이 아닌 혜택은 하나만)
    │   ├── Common.tsx        Loading, ErrorBox, Panel, Amount, Delta, Meter, StatTile
    │   ├── LedgerTabs.tsx    장부 선택 탭 (전체 포함 여부 선택)
    │   ├── MonthPicker.tsx   이전/다음 달 이동
    │   ├── Modal.tsx
    │   ├── Toast.tsx         토스트 (오류 6초, 일반 2.8초, 최대 4개)
    │   └── charts/           라이브러리 없이 SVG로 그린 LineChart, ColumnChart, useWidth·niceScale
    └── pages/                화면 하나당 파일 하나 (아래 화면 명세)
```

## 구조와 규칙

### 인증과 라우팅

`App.tsx`가 `GET /api/session`을 먼저 확인합니다. 인증이 켜져 있고 세션이 없으면 모든 경로에서 로그인 화면만 보여줍니다.
GraphQL 요청이 401을 받으면 `client.ts`가 `cashbook:unauthorized` 이벤트를 보내고, `App`이 세션을 다시 읽어 로그인 화면으로 돌아갑니다.
인증된 상태에서는 `Layout` 안에 화면을 그리고, 알 수 없는 경로는 `/`로 보냅니다.

### 데이터 읽기·쓰기 (`api/`)

- **기준 데이터** `useRefData()`: 앱 정보·장부·카테고리·결제수단·계좌·구성원을 한 번에 읽고(보관 항목 포함, 60초 캐시) `categoryById`, `paymentMethodById`, `categoriesOf(ledgerId)`, `activePaymentMethods` 같은 맵을 만들어 줍니다. 거래 목록 등은 id만 받아 오고 이름·색은 여기서 찾습니다.
- **조회** `useGql(key, query, variables)`: TanStack Query 래퍼. `placeholderData`로 이전 결과를 유지해 월·필터를 바꿀 때 화면이 비지 않습니다. 기본 `staleTime` 10초, 창 포커스 시 다시 읽기.
- **변경** `useMutate().run(query, variables)`: 성공하면 `queryClient.invalidateQueries()`로 **모든 조회를 다시 읽습니다** (데이터가 작아 범위를 좁히지 않음). 실패하면 토스트를 띄우고 예외 대신 `undefined`를 돌려주므로, 모달 닫기처럼 성공해야 하는 후속 동작은 결과를 확인한 뒤 합니다.
- GraphQL 문서는 모두 `queries.ts`에 둡니다. 필드 묶음(`TX_FIELDS`, `PAYMENT_METHOD_FIELDS`, `STATEMENT_FIELDS`, `CARD_REPORT_FIELDS` …)을 조합해 화면마다 필요한 필드만 가져옵니다. 응답 타입은 `types.ts`에 손으로 맞춥니다.

### URL 상태 (`lib/month.ts`)

월·장부·필터처럼 공유·새로고침 후에도 유지되어야 하는 상태는 쿼리 파라미터에 둡니다.

- `useMonthParam()`: `?month=YYYY-MM`, 없거나 형식이 틀리면 이번 달.
- `useQueryParam(name, fallback)`: 값이 기본값이면 파라미터를 지웁니다.
- 두 훅 모두 `window.location.search`에서 시작해 갱신하고 `replace`로 기록합니다. 한 핸들러에서 여러 파라미터를 바꿔도 서로 덮어쓰지 않게 하기 위해서입니다.

### 거래 그리드 (`TransactionGrid`)

가계부·카드 상세·카드별 내역이 같은 컴포넌트를 쓰고, 화면마다 `columns`로 열을 고릅니다
(`date` `amount` `category` `paymentMethod` `description` `member` `memo` `ledger` `benefits` `benefitAmount`. `benefitAmount`만 계산값이라 편집 불가).

- 방향키로 셀 이동, Enter/F2로 편집 시작.
- 편집 중 Enter → 저장 후 아래 칸, Tab / Shift+Tab → 저장 후 다음 / 이전 칸, Esc → 취소.
- **blur로 편집이 끝나면 포커스를 옮기지 않습니다.** 다른 칸이나 입력칸을 누른 사용자의 입력을 뺏지 않기 위해서입니다.
- `newRow`를 주면 맨 아래에 입력 행이 생깁니다. Enter를 연달아 눌러도 한 건만 저장합니다. 내용 칸은 `DescriptionInput` 자동완성으로 빈 분류를 채웁니다.
- 예정 행에는 `예정` 배지(날짜가 지났으면 `! 미확정`)와 확정(예정 금액·날짜로)·건너뛰기 버튼, 건너뛴 행에는 복원 버튼이 붙습니다.
- `selection`을 주면 행 번호 자리에 체크박스가 생기고(머리글은 보이는 행 모두 선택, Shift+클릭은 범위 선택) 새 행 입력은 숨깁니다.

### 화면 스타일·차트

- `styles.css` 하나에 CSS 변수로 역할별 색(`--accent`, `--series-context`, `--good`, `--bad`, `--warn`, `--planned-bg` …)을 정의하고, `prefers-color-scheme: dark`에서 같은 이름으로 다크 값을 덮어씁니다.
- 반응형 구간: 1320px, 1100px, 900px, 600px. 결제 입력 화면은 휴대폰에서 쓰는 것을 전제로 합니다.
- 차트는 dataviz 기준을 따릅니다. 이번 달·선택 항목 = accent 파랑, 비교 맥락(평균·지난달) = 회색, 2px 선, 툴팁과 표 보기 제공. 새 차트도 `components/charts/`의 규칙을 따릅니다.

## 화면 명세

| 메뉴 | 경로 | 파일 | URL 파라미터 |
|---|---|---|---|
| 대시보드 | `/` | `pages/Dashboard.tsx` | `month`, `ledger`, `avg` |
| 가계부 | `/transactions` | `pages/Transactions.tsx` | `month`, `ledger`, `category`, `pm`, `member`, `status` |
| 결제 입력 | `/entry` | `pages/Entry.tsx` | — |
| 카드 | `/cards` | `pages/Cards.tsx` | — |
| (카드 상세) | `/cards/:id` | `pages/CardDetail.tsx` | `month`, `edit` |
| 카드별 내역 | `/card-transactions` | `pages/CardTransactions.tsx` | `month`, `card`, `mode` |
| 은행 잔고 | `/accounts` | `pages/Accounts.tsx` | — |
| 지출 계획 | `/plan` | `pages/Plan.tsx` | `ledger` |
| 설정 | `/settings` | `pages/Settings.tsx` | — |
| (로그인) | 모든 경로 | `pages/Login.tsx` | — |

공통 파라미터: `month`는 `YYYY-MM`(기본 이번 달). `ledger`는 `default`(기본 장부, 생략 시) · `all`(전체) · 장부 id.

### 대시보드 `/`

이번 달 소비를 평균·예산·계획과 비교하고, 여유자금과 다가오는 카드 결제를 한눈에 봅니다.

| 영역 | 내용 |
|---|---|
| 상단 | 장부 탭(전체 포함), 월 선택, 평균 기간(`avg`, 기본 6개월) |
| 지표 | 이번 달 소비 vs N개월 평균, 예정 포함 예상 소비, 예산 대비(예정 포함 미터), 월잔액(계획 기준), 수입 |
| 일별 누적 소비 | 선 차트: 이번 달 소비(오늘까지), 예정 포함(예정이 있을 때), N개월 평균. 표로 보기 전환 |
| 여유자금 | 현재 / 예정 반영 후 여유자금과 계산 내역, 다가오는 카드 결제 목록, 정산 일정 없는 카드 경고. "잔고 입력" → `/accounts` |
| 카테고리별 이번 달 vs 평균 | 막대(옅은 부분 = 예정) + 평균 세로선 |
| 예정 지출 | 예정 거래 목록. 미확정(날짜 지남) 표시, 확정(그 금액·날짜로) / 건너뛰기 |
| 월별 소비 추이 | 최근 12개월 막대 차트 + 12개월 평균선. 막대를 누르면 그 달로 이동 |
| 결제수단별 이번 달 사용액 | 결제수단별 확정·예정 금액 |
| 구성원 · 지출 계획 | 계획 수입·소비·기타 수입·배분·월잔액, 구성원별 지출과 용돈 잔액 (전체 장부에서는 월잔액 대신 안내) |
| 카드 실적 현황 | 카드별 이번 달 실적의 목표 대비 진행, 혜택 사용 |

- 데이터: `SUMMARY_QUERY`, `FUNDS_QUERY`, `CARDS_MONTHLY_QUERY`, `TRANSACTIONS_QUERY`(예정)
- 변경: `CONFIRM_TX`, `SKIP_TX`

### 가계부 `/transactions`

스프레드시트 월 시트처럼 거래를 보고 바로 고칩니다.

| 영역 | 내용 |
|---|---|
| 필터 | 장부 탭, 월, 항목(`미분류` = `none`), 결제수단, 구성원, 상태(`active` 확정+예정 / `CONFIRMED` / `PLANNED` / `SKIPPED`), 내용 검색(300ms 지연, URL에 남기지 않음) |
| 거래 그리드 | 열: 날짜·금액·항목·결제수단·내용·구성원·메모 (+ 전체 장부일 때 장부). 필터가 없을 때만 맨 아래 새 행 입력 |
| 장부 이동 | 검색 옆 `선택` → 체크박스와 옮길 장부·`이동` 버튼. 지금 목록에 보이는 체크한 거래만 옮긴다. 옮길 장부에 이름·종류가 같은 항목이 있으면 그 항목으로, 없으면 미분류 |
| 오른쪽 요약 | 이번 달 요약(총 사용 금액, 합계 제외, 계획 수입, 배분 항목, 월잔액, 구성원 지출 / 용돈), 소비 총액(항목별), 결제수단별 합계 |

- 데이터: `TRANSACTIONS_QUERY`, `SUMMARY_QUERY`
- 변경(그리드): `CREATE_TX`, `UPDATE_TX`, `DELETE_TX`, `CONFIRM_TX`, `SKIP_TX`, `RESTORE_TX`
- 변경(장부 이동): `MOVE_TXS` (`moveTransactions(ids, ledgerId)`, 한 트랜잭션)

### 결제 입력 `/entry`

휴대폰에서도 한 손으로 입력할 수 있는 폼 화면입니다.

| 영역 | 내용 |
|---|---|
| 새 결제 | 날짜, 금액(환불 체크), 내용(가맹점 자동완성 → 장부·항목·결제수단·구성원·혜택 자동 채움), 장부, 항목, 결제수단, 구성원, 메모, 할부, 혜택, 실적 제외, 예정으로 저장. 저장 후 날짜·장부·결제수단·상태는 유지 |
| 카드 승인 문자로 입력 | 붙여넣은 문자를 **해석**(`parseMessage` → 폼 채우기, 끝 4자리 → 카드사로 결제수단 찾기)하거나 **바로 저장**(`ingestTransaction`, 중복·예정 매칭 결과를 토스트로 표시) |
| 최근 내역 | 오늘까지 확정된 결제, 최신순 |

- 데이터: `PARSE_MESSAGE_QUERY`, `SUGGESTIONS_QUERY`(자동완성), `TRANSACTIONS_QUERY`
- 변경: `CREATE_TX`, `INGEST`

### 카드 `/cards`

| 영역 | 내용 |
|---|---|
| 카드 목록 | 카드별 다음 결제일·금액, 이번 달 실적 미터, 결제 계좌, 마지막 사용일. 누르면 카드 상세 |
| 보관 카드 보기 | 체크하면 보관한 카드도 표시, 복원 가능 |
| 카드 추가 | `CardForm` 모달. 결제일만 고르고 "결제일로 자동 추정"하면 마감일·마감월을 채움 |
| 삭제 | 거래가 있으면 보관, 없으면 삭제 (결과를 토스트로 표시) |

- 데이터: `CARD_LIST_QUERY`, `CARDS_MONTHLY_QUERY`(이번 달 고정)
- 변경: `CREATE_PM`, `SUGGEST_BILLING`(CardForm), `DELETE_PM`, `RESTORE_PM`

### 카드 상세 `/cards/:id`

| 영역 | 내용 |
|---|---|
| 카드 정보 | 카드사·상품명·카드번호·결제 계좌·정산 일정·전월실적 목표·연회비·유효기간·자동입력 별칭·거래 수·메모, 삭제(보관)·복원. `?edit=1`이면 `CardForm` 편집 모드 |
| 결제 / 결제 예정 금액 | 다음 결제, 청구서(결제월)별 이용기간·확정·예정 금액·상태(이용 전 / 이용 중 / 결제 예정 / 결제됨). 청구서를 누르면 결제분 내역 모달(할부 회차 포함) |
| 월 실적 · 혜택 | 전월실적 목표 대비 미터, 혜택 한도 사용 미터(통합 한도에서 차감된 혜택만), 종류별 혜택 합계(원·P·마일), 혜택 항목별 사용·잔여·한도 방식·중복 적용, 다음 구간까지 남은 금액 |
| 카드 거래 | 그리드. 열: 날짜·금액·항목·내용·혜택·혜택액·구성원·메모. 혜택 칸에서 거래별 혜택 항목 지정 |

- 데이터: `CARD_DETAIL_QUERY`, `STATEMENT_LINES_QUERY`, `TRANSACTIONS_QUERY`
- 변경: `UPDATE_PM`(CardForm), `DELETE_PM`, `RESTORE_PM`, 그리드 거래 변경

### 카드별 내역 `/card-transactions`

카드 시트처럼 카드별 월 이용 내역과 혜택액을 봅니다.

| 영역 | 내용 |
|---|---|
| 상단 | 월 선택, 기준 전환(`mode=usage` 이용월 / `statement` 결제월) |
| 왼쪽 카드 목록 | 그 달 카드별 이용 합계. `card` 파라미터가 없으면 이용이 있는 첫 카드 선택 |
| 오른쪽 | 선택 카드 실적·혜택 요약 + 이용월 기준이면 이용 내역 그리드(혜택·혜택액·장부), 결제월 기준이면 그 달 결제분(이용기간 안내, 할부 회차) |

- 데이터: `CARDS_MONTHLY_QUERY`, `TRANSACTIONS_QUERY`, `STATEMENT_LINES_QUERY`

### 은행 잔고 `/accounts`

| 영역 | 내용 |
|---|---|
| 지표 | 여유자금 계산에 포함된 잔고(계좌 수), 전체 잔고(적금·투자 포함), 현재 여유자금 |
| 계좌 표 | 계좌별 잔고·기준일·여유자금 포함 여부. 행에서 새 잔고와 기준일을 바로 기록, 편집, 이력 보기. 보관 계좌 보기 |
| 계좌 추가/편집 모달 | 이름·은행·종류·계좌번호·잔고·기준일·여유자금 포함·이율·만기·메모. 편집 때는 잔고나 기준일을 실제로 바꿨을 때만 잔고 기록을 남김 |
| 잔고 이력 모달 | 최근 60건, 기록 삭제 |

- 데이터: `FUNDS_QUERY`, `ACCOUNT_HISTORY_QUERY`
- 변경: `CREATE_ACCOUNT`, `UPDATE_ACCOUNT`, `DELETE_ACCOUNT`, `UPDATE_BALANCE`, `DELETE_BALANCE`

### 지출 계획 `/plan`

장부별 예산과 고정 수입·배분, 반복 지출을 관리합니다. 장부 탭에 "전체"는 없습니다.

| 영역 | 내용 |
|---|---|
| 카테고리 예산 | 카테고리별 월 예산·고정비·합계 제외를 표에서 바로 수정, 이번 달(확정+예정) vs 평균, 순서 위/아래, 추가·삭제(사용 중이면 보관) |
| 월 수입 · 배분 | 수입(월급), 배분(용돈·저축·적금), 구성원 용돈 항목 |
| 반복 지출 | 규칙 목록(다음 날짜, 매월 합계, 목록에서 바로 켜기/끄기)과 추가/편집 모달(내용·금액·항목·결제수단·날짜(말일 가능)·반복(매월 / N개월마다 / 매년)·시작~종료 월·구성원·자동입력 매칭 키워드·메모·전월실적 제외). 저장하면 이번 달 + `planMonthsAhead`개월치 예정 거래 생성 |

- 데이터: `SUMMARY_QUERY`, `PLAN_ITEMS_QUERY`, `RECURRING_QUERY`
- 변경: `CREATE/UPDATE/DELETE_CATEGORY`, `REORDER_CATEGORIES`, `CREATE/UPDATE/DELETE_PLAN_ITEM`, `CREATE/UPDATE/DELETE_RULE`

### 설정 `/settings`

| 영역 | 내용 |
|---|---|
| 장부 | 추가·이름 변경·삭제, 기본 장부 지정 (자동 입력과 대시보드의 기본값) |
| 카드 외 결제수단 | 현금·계좌이체 등 추가/편집(`CardForm`), 연결 계좌 지정, 삭제·복원 |
| 자동 입력 API 토큰 | 이름·권한(자동 입력 전용 / 전체 API)으로 발급, 원문은 발급 모달에서 한 번만 표시, 폐기 |
| 자동 입력 API 사용법 | 문자 원문 / JSON / 아이폰 단축어 / 안드로이드(자동 입력 앱·문자 전달 앱·MacroDroid·Tasker) / GraphQL 단계별 안내 |

- 데이터: `TOKENS_QUERY`
- 변경: `CREATE/UPDATE/DELETE_LEDGER`, `CREATE_PM`·`UPDATE_PM`(CardForm), `DELETE_PM`, `RESTORE_PM`, `CREATE_TOKEN`, `DELETE_TOKEN`

### 로그인

인증이 켜져 있고 세션이 없으면 경로와 관계없이 표시됩니다. `POST /api/login` 성공 시 세션을 다시 읽어 원래 화면으로 돌아갑니다.

## 새 화면을 추가할 때

1. `src/pages/`에 파일을 만들고 `App.tsx` 라우트와 `Layout.tsx`의 `NAV`에 추가합니다.
2. 쿼리·뮤테이션은 `api/queries.ts`, 응답 타입은 `api/types.ts`에 둡니다. 서버 스키마가 바뀌면 [server/README.md](../server/README.md)도 맞춥니다.
3. 월·장부·필터처럼 링크로 공유될 상태는 `useMonthParam`/`useQueryParam`을 씁니다.
4. 주요 흐름이면 `e2e/smoke.mjs`에 단계를 추가합니다.
5. 이 문서의 화면 명세를 갱신합니다.
