# CLAUDE.md

이 저장소에서 작업할 때의 안내. 사용자 문서는 `README.md`.

## 구성

- `server/` — Rust 2024, Rocket 0.5 + async-graphql 7 + SeaORM 2 + SQLite 또는 MySQL(`DATABASE_URL`). 바이너리 이름 `cashbook`.
- `web/` — React 19 + TypeScript + Vite. `npm run build` 결과(`web/dist`)를 Rocket이 `/`에서 서빙(SPA fallback).
- `tools/` — 스프레드시트 xlsx → `import/normalized.json` 변환(Python, openpyxl). `import/`는 개인 금융 데이터라 git 제외.
- `k8s/` — 서버 이미지(`docker/`, 저장소 루트 컨텍스트 + 허용 목록 dockerignore, Docker Hub `rouddy/cashbook`에 `docker/build_and_push.sh`로 amd64+arm64 푸시 — 태그는 master면 Cargo `version`, 그 밖은 `version-<short sha>`), 앱 kustomization(`app/`), 데이터 PVC(`setup/`, 앱과 분리), 웹 빌드 PVC 업로드·DB 백업/복원 스크립트. SQLite라 replicas 1 + Recreate 고정. 공개 주소 `https://cashbook.ster.pe.kr`(Traefik Ingress + cert-manager Let's Encrypt, `app/traefik` 컴포넌트가 HTTP→HTTPS). 사용법은 `k8s/README.md`.
- `.circleci/config.yml` — master 푸시에서만 병렬: 서버 이미지(태그 규칙 `k8s/docker/image_tag.sh`, Docker Hub에 태그가 있으면 건너뜀, amd64·arm64 네이티브 빌드 → digest를 태그 하나로 묶음), 서명한 릴리스 APK, 웹 dist zip(`cashbook_web_<package.json version>.zip`)을 아티팩트로.
- `android/` — 카드 승인 문자·앱 알림을 규칙(보내는 번호/앱 + 형식 템플릿)으로 골라 `/api/ingest`로 올리는 앱. Kotlin + Compose, AGP 9(내장 Kotlin, KGP는 루트에서 `kotlin-android` apply false로 버전 고정), DataStore(JSON) + WorkManager. 구조·형식 문법은 `android/README.md`.

## 명령

```bash
cd server && cargo run                          # 127.0.0.1:8000, server 디렉터리에서 실행 (DB·web 기본 경로 기준)
cd server && cargo run -- import <json> [--replace]
cd server && cargo run -- copy-db <원본 URL> <대상 URL>   # SQLite ↔ MySQL 데이터 복사 (대상은 빈 DB)
cd server && cargo test
cd web && npm run dev | npm run build | npx tsc -b | npx oxlint
cd web && npm run e2e                           # 테스트용 DB 사본 서버(8001) 필요 — README 참고, 실제 DB로 돌리지 말 것
cd android && ./gradlew testDebugUnitTest lintDebug assembleDebug   # 에뮬레이터 시험은 android/README.md (adb reverse로 8001 사본 서버)
```

## 서버 구조

- `entity/` — 테이블당 하나의 SeaORM 엔티티(DeriveEntityModel). 대부분 `async_graphql::SimpleObject`를 같이 derive해서 GraphQL 타입으로 그대로 노출하고, 관계/계산 필드는 `graphql/*.rs`의 `#[ComplexObject] impl entity::Model`에 둔다. 문자열 enum은 `entity/enums.rs`(DeriveActiveEnum + graphql Enum).
- `db/migration.rs` — 손으로 쓴 마이그레이션. **스키마를 바꾸면 기존 마이그레이션을 고치지 말고 새 마이그레이션을 추가하고 엔티티도 맞춘다** (타입 매핑: i64→big_integer, NaiveDate→date, DateTimeUtc→timestamp_with_time_zone, Option→*_null). SQLite·MySQL 둘 다 돌아야 한다: `string()`은 MySQL varchar(255)라 긴 글은 `text_null`.
- `domain/` — GraphQL과 분리된 계산·처리. 순수 함수 위주이고 단위 테스트가 같이 있다.
  - `billing.rs` 카드 정산 주기(결제월 M 청구서의 마감일 = (M+offset)월 closing_day, 결제일 주말 보정, 할부 분할)
  - `benefits.rs` / `card_report.rs` 전월실적·혜택(건별 → 항목 월한도 → 통합한도 날짜순 누적)
  - `summary.rs` 월 요약(시트와 같은 소비·월잔액 규칙), `funds.rs` 여유자금, `recurring.rs` 예정 거래 생성
  - `ingest.rs` 자동 입력(중복 방지 → 결제수단 해석 → 분류 추천 → 예정 매칭), `sms.rs` 승인 문자 파서
  - `import.rs` 정규화 JSON 가져오기
  - `split.rs` 거래 분할(한 결제를 두 장부에 — 자식이 `split_parent_id`로 부모를 가리킴, 결제 속성 동기화·금액 재분배·합치기)
  - `legacy_plan.rs` 예전 지출계획(월 수입·배분·합계 제외)을 정기수익 반복 수입·장부 이체 반복·장부로 옮기는 규칙 (마이그레이션 `m20261120_000007`과 `import.rs`가 공용)
- `graphql/` — 도메인별 Query/Mutation을 `MergedObject`로 합침. 요청마다 `Lookup`(작은 테이블 캐시)을 `request.data`로 넣는다. 뮤테이션 후엔 `lookup(ctx).invalidate()`.
- `routes.rs` — `/api/graphql`(POST JSON, `FullAccess` 가드), `/api/ingest`(JSON·text/plain, `Auth` 가드), `/api/ingest/options`(GET, `Auth` — 안드로이드 규칙의 장부·카테고리·결제수단 선택 목록, 이름만), 세션/로그인, SPA 서빙. `auth.rs` — 비밀번호 세션(암호화 쿠키, SameSite=Strict) + API 토큰(SHA-256 해시, 권한 `INGEST`(기본, ingest와 규칙용 이름 목록 `/api/ingest/options`만)/`FULL`). `Auth` 가드는 Origin≠Host 요청을 403으로 막고, 인증이 꺼진 로컬 모드에서는 localhost Host만 받는다. `CASHBOOK_PASSWORD`가 없으면 인증이 꺼지고, 그때 비루프백 주소로는 시작을 거부한다. 403 사유는 `ForbiddenReason`으로 요청 캐시에 남겨 catcher가 JSON으로 돌려준다.

## 놓치기 쉬운 것

- 금액 부호: 지출 카테고리는 +지출/−환불, 수입 카테고리는 +수입. 카드 청구·결제수단 합계는 `summary::outflow()`(수입이면 부호 반전, 장부 이체는 0)를 쓴다.
- 장부 이체: `transfer_ledger_id`가 있는 거래 한 행 = `ledger_id`에서 그 장부로 옮긴 돈(항목·결제수단 없음, 반복 규칙도 같은 컬럼). 소비·수입이 아니라 `summary::classify(tx, cats, ledger_id)`가 보는 장부 기준으로 받은/보낸 이체로 나누고, 전체 장부 합계·카드·여유자금·추천(`ingest::history`)에서는 뺀다. 장부를 정한 조회(`transactions` 필터, `monthlySummary`, `recurringRules`)는 받은 이체(`transfer_ledger_id = 장부`)도 함께 가져온다 — 장부별로 거래를 읽는 경로를 새로 만들면 받은 이체를 빠뜨리지 않는다. 월잔액 = 수입 + 받은 이체 − 소비 − 보낸 이체 − 배분. 지출계획의 월 수입(`INCOME`)과 카테고리 '합계 제외'는 없어졌다(정기수익 반복 수입, 장부 분리로 대체). 웹 표시는 `lib/transfer.ts`.
- 반복 규칙이 이번 달 이후에 만든 항목은 상태와 관계없이 삭제하면 SKIPPED로 남긴다(유니크 `(recurring_rule_id, recurring_month)` 자리를 비우면 `ensure_planned`가 다시 만든다). 규칙 수정 시 이번 달 이후 PLANNED만 다시 만든다.
- 사용 중인 카테고리/결제수단 삭제는 보관(archived) 처리. 결제수단 보관 시 그 결제수단의 반복 규칙을 끄고 이번 달 이후 예정분을 지운다.
- 여유자금은 결제 계좌가 여유자금 제외 계좌인 카드의 대금·예정 출금을 빼지 않는다(`CardDue.counted`). 혜택 환불은 그 달에 받은 혜택까지만 되돌린다.
- 거래 분할: 부모 + 자식(`split_parent_id`) 금액의 합이 실제 결제. 확정 거래만 나눈다(예정은 반복 규칙이 지우고 다시 만들어 짝이 깨짐). 묶음의 날짜·결제수단·할부·실적 제외·상태는 `split::sync_payment_fields`로 맞추고, 금액을 고치면 짝이 차액을 맡는다(`split::rebalance`). 혜택 지정은 부모에 두고(`benefit_owner`), 카드 실적·혜택은 묶음을 한 건으로 계산한다(`card_report::card_txs`). 묶음 거래는 장부 이동·상태 변경을 막고, 삭제는 합친 뒤 결제 전체를 지운다 — 거래를 지우거나 상태를 바꾸는 경로를 새로 만들면 분할 묶음을 먼저 처리한다. 웹의 분할 판단·합계·금액 규칙은 `lib/split.ts`(서버 `split::check_part`와 같게).
- 카드 혜택: 거래에 고른 혜택은 모두 더하고, `stackable`이 아닌 혜택은 거래당 하나만(`benefits::stacking_conflict`, UI는 `lib/benefits.ts`의 `toggleBenefit`). 혜택액은 `kind` 단위(할인 원·포인트 P·마일리지 마일)라 종류별로 합친다(`discountTotal`/`pointTotal`/`mileageTotal`, `benefitTotal` = 할인+포인트). 통합 한도 사용은 `limitUsed`(한도 없는·별도 한도 혜택 제외).
- 프론트는 뮤테이션 성공 시 `queryClient.invalidateQueries()`로 전부 다시 읽는다(`api/hooks.ts`의 `useMutate`). `run()`은 실패해도 던지지 않고 토스트 후 `undefined`를 돌려주므로, 성공해야 하는 후속 동작은 결과를 확인한다.
- URL 쿼리 상태(`lib/month.ts`의 `useMonthParam`/`useQueryParam`)는 한 핸들러에서 여러 개를 바꿀 수 있게 항상 `window.location.search`에서 시작해 갱신한다.
- 가계부 그리드 편집은 blur로 끝나면 포커스를 옮기지 않는다(다른 칸을 누른 사용자 입력을 뺏지 않기). Enter/Tab/Esc일 때만 다음 칸으로 이동.
- 안드로이드 앱은 서버 `ingest`에 `text`(원문)를 항상 보내고 형식에서 뽑은 값만 명시 필드로 넣는다(서버 중복 방지는 원문 해시). 템플릿 문법을 바꾸면 `MessageFormatTest`와 `android/README.md` 표를 같이 고친다. 보낼 `card`는 `RuleMatcher.cardChoice`(카드별 결제수단 → 규칙 결제수단 → `{끝자리}` → `{카드}`) 한 곳에서 정한다. `/api/ingest/options` 응답 모양을 바꾸면 앱의 `ServerOptions`와 `CashbookApiTest.parseOptions`도 맞춘다.
- DB 이식성: `Expr::cust` SQL은 두 DB 공통 함수만, 나머지는 `db::sql`(`char_length`, `sum_i64` — MySQL SUM은 DECIMAL). ORDER BY 없는 순서는 DB마다 다르므로 결과 순서가 보이면 정렬을 명시(LIMIT 앞은 동점까지). 테스트는 인메모리 SQLite라 MySQL 차이는 잡지 못한다 — 쿼리를 바꾸면 MySQL에서도 확인.
- 차트 색은 dataviz 기준(이번 달=accent 파랑, 비교 맥락=회색, 2px 선, 툴팁·표 보기). 새 차트도 같은 규칙을 따른다.
