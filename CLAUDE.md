# CLAUDE.md

이 저장소에서 작업할 때의 안내. 사용자 문서는 `README.md`.

## 구성

- `server/` — Rust 2024, Rocket 0.5 + async-graphql 7 + SeaORM 2 + SQLite. 바이너리 이름 `cashbook`.
- `web/` — React 19 + TypeScript + Vite. `npm run build` 결과(`web/dist`)를 Rocket이 `/`에서 서빙(SPA fallback).
- `tools/` — 스프레드시트 xlsx → `import/normalized.json` 변환(Python, openpyxl). `import/`는 개인 금융 데이터라 git 제외.
- `android/` — 카드 승인 문자·앱 알림을 규칙(보내는 번호/앱 + 형식 템플릿)으로 골라 `/api/ingest`로 올리는 앱. Kotlin + Compose, AGP 9(내장 Kotlin, KGP는 루트에서 `kotlin-android` apply false로 버전 고정), DataStore(JSON) + WorkManager. 구조·형식 문법은 `android/README.md`.

## 명령

```bash
cd server && cargo run                          # 127.0.0.1:8000, server 디렉터리에서 실행 (DB·web 기본 경로 기준)
cd server && cargo run -- import <json> [--replace]
cd server && cargo test
cd web && npm run dev | npm run build | npx tsc -b | npx oxlint
cd web && npm run e2e                           # 테스트용 DB 사본 서버(8001) 필요 — README 참고, 실제 DB로 돌리지 말 것
cd android && ./gradlew testDebugUnitTest lintDebug assembleDebug   # 에뮬레이터 시험은 android/README.md (adb reverse로 8001 사본 서버)
```

## 서버 구조

- `entity/` — 테이블당 하나의 SeaORM 엔티티(DeriveEntityModel). 대부분 `async_graphql::SimpleObject`를 같이 derive해서 GraphQL 타입으로 그대로 노출하고, 관계/계산 필드는 `graphql/*.rs`의 `#[ComplexObject] impl entity::Model`에 둔다. 문자열 enum은 `entity/enums.rs`(DeriveActiveEnum + graphql Enum).
- `db/migration.rs` — 손으로 쓴 마이그레이션. **스키마를 바꾸면 기존 마이그레이션을 고치지 말고 새 마이그레이션을 추가하고 엔티티도 맞춘다** (타입 매핑: i64→big_integer, NaiveDate→date, DateTimeUtc→timestamp_with_time_zone, Option→*_null).
- `domain/` — GraphQL과 분리된 계산·처리. 순수 함수 위주이고 단위 테스트가 같이 있다.
  - `billing.rs` 카드 정산 주기(결제월 M 청구서의 마감일 = (M+offset)월 closing_day, 결제일 주말 보정, 할부 분할)
  - `benefits.rs` / `card_report.rs` 전월실적·혜택(건별 → 항목 월한도 → 통합한도 날짜순 누적)
  - `summary.rs` 월 요약(시트와 같은 소비·월잔액 규칙), `funds.rs` 여유자금, `recurring.rs` 예정 거래 생성
  - `ingest.rs` 자동 입력(중복 방지 → 결제수단 해석 → 분류 추천 → 예정 매칭), `sms.rs` 승인 문자 파서
  - `import.rs` 정규화 JSON 가져오기
- `graphql/` — 도메인별 Query/Mutation을 `MergedObject`로 합침. 요청마다 `Lookup`(작은 테이블 캐시)을 `request.data`로 넣는다. 뮤테이션 후엔 `lookup(ctx).invalidate()`.
- `routes.rs` — `/api/graphql`(POST JSON, `FullAccess` 가드), `/api/ingest`(JSON·text/plain, `Auth` 가드), 세션/로그인, SPA 서빙. `auth.rs` — 비밀번호 세션(암호화 쿠키, SameSite=Strict) + API 토큰(SHA-256 해시, 권한 `INGEST`(기본, ingest만)/`FULL`). `Auth` 가드는 Origin≠Host 요청을 403으로 막고, 인증이 꺼진 로컬 모드에서는 localhost Host만 받는다. `CASHBOOK_PASSWORD`가 없으면 인증이 꺼지고, 그때 비루프백 주소로는 시작을 거부한다. 403 사유는 `ForbiddenReason`으로 요청 캐시에 남겨 catcher가 JSON으로 돌려준다.

## 놓치기 쉬운 것

- 금액 부호: 지출 카테고리는 +지출/−환불, 수입 카테고리는 +수입. 카드 청구·결제수단 합계는 `summary::outflow()`(수입이면 부호 반전)를 쓴다.
- 반복 규칙이 이번 달 이후에 만든 항목은 상태와 관계없이 삭제하면 SKIPPED로 남긴다(유니크 `(recurring_rule_id, recurring_month)` 자리를 비우면 `ensure_planned`가 다시 만든다). 규칙 수정 시 이번 달 이후 PLANNED만 다시 만든다.
- 사용 중인 카테고리/결제수단 삭제는 보관(archived) 처리. 결제수단 보관 시 그 결제수단의 반복 규칙을 끄고 이번 달 이후 예정분을 지운다.
- 여유자금은 결제 계좌가 여유자금 제외 계좌인 카드의 대금·예정 출금을 빼지 않는다(`CardDue.counted`). 혜택 환불은 그 달에 받은 혜택까지만 되돌린다.
- 카드 혜택: 거래에 고른 혜택은 모두 더하고, `stackable`이 아닌 혜택은 거래당 하나만(`benefits::stacking_conflict`, UI는 `lib/benefits.ts`의 `toggleBenefit`). 혜택액은 `kind` 단위(할인 원·포인트 P·마일리지 마일)라 종류별로 합친다(`discountTotal`/`pointTotal`/`mileageTotal`, `benefitTotal` = 할인+포인트). 통합 한도 사용은 `limitUsed`(한도 없는·별도 한도 혜택 제외).
- 프론트는 뮤테이션 성공 시 `queryClient.invalidateQueries()`로 전부 다시 읽는다(`api/hooks.ts`의 `useMutate`). `run()`은 실패해도 던지지 않고 토스트 후 `undefined`를 돌려주므로, 성공해야 하는 후속 동작은 결과를 확인한다.
- URL 쿼리 상태(`lib/month.ts`의 `useMonthParam`/`useQueryParam`)는 한 핸들러에서 여러 개를 바꿀 수 있게 항상 `window.location.search`에서 시작해 갱신한다.
- 가계부 그리드 편집은 blur로 끝나면 포커스를 옮기지 않는다(다른 칸을 누른 사용자 입력을 뺏지 않기). Enter/Tab/Esc일 때만 다음 칸으로 이동.
- 안드로이드 앱은 서버 `ingest`에 `text`(원문)를 항상 보내고 형식에서 뽑은 값만 명시 필드로 넣는다(서버 중복 방지는 원문 해시). 템플릿 문법을 바꾸면 `MessageFormatTest`와 `android/README.md` 표를 같이 고친다.
- 차트 색은 dataviz 기준(이번 달=accent 파랑, 비교 맥락=회색, 2px 선, 툴팁·표 보기). 새 차트도 같은 규칙을 따른다.
