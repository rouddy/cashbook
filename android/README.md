# 가계부 자동 입력 (Android)

카드 승인 **문자**나 결제 앱의 **푸시 알림**을 받으면, 미리 정한 규칙에 맞는 것만 골라 가계부 서버의
자동 입력 API(`POST /api/ingest`)로 바로 올리는 안드로이드 앱입니다.
휴대폰에서 바로 보는 **대시보드**, 지출·수입·장부 이체를 넣는 **수동 입력**, 들어온 거래를 이어서 보는 **최근 내역**도 있습니다
(가계부 GraphQL `POST /api/graphql`, 자동 입력과 같은 토큰).

- Kotlin · Jetpack Compose(Material3) · WorkManager · DataStore · kotlinx.serialization
- minSdk 29 (Android 10), target/compile SDK 37, AGP 9.5 · Gradle 9.6 · Kotlin 2.4

## 빌드·설치

```bash
cd android
./gradlew assembleDebug                 # → app/build/outputs/apk/debug/cashbook_<versionName>_<versionCode>.apk
./gradlew installDebug                  # 연결된 기기/에뮬레이터에 설치
./gradlew testDebugUnitTest lintDebug   # 단위 테스트(형식 해석·규칙·응답 처리) + 린트
./gradlew assembleRelease               # → app/build/outputs/apk/release/cashbook_<versionName>_<versionCode>.apk
```

`ANDROID_HOME`(또는 `local.properties`의 `sdk.dir`)에 Android SDK가 있어야 합니다. Android Studio로 `android/` 폴더를 열어도 됩니다.

릴리스 서명은 환경 변수(또는 `~/.gradle/gradle.properties`의 같은 이름 속성) `CASHBOOK_KEYSTORE_FILE`, `CASHBOOK_KEYSTORE_PASSWORD`,
`CASHBOOK_KEY_ALIAS`, `CASHBOOK_KEY_PASSWORD`로 합니다. 없으면 릴리스 APK는 서명 없이 만들어져 설치할 수 없습니다.
CircleCI(`.circleci/config.yml`의 `android-apk`)는 키스토어를 `CASHBOOK_KEYSTORE_BASE64`로 받아 서명한 릴리스 APK를 아티팩트로 남깁니다.
이미 설치한 앱과 서명 키가 다르면 업데이트 설치가 거부되므로(지우고 다시 설치하면 규칙·설정이 사라짐), 키는 한 번 정해 계속 씁니다.

## 화면

하단 메뉴로 네 화면을 오갑니다. 화면마다 스크롤·입력 중인 값은 다른 화면에 다녀와도 남습니다.

| 화면 | 내용 |
|---|---|
| **대시보드** | 달(◀ ▶)·장부(기본 장부/장부별/전체, 마지막 선택을 기억) 기준으로 이번 달 소비(평균 대비)·수입·월잔액(장부를 고르면, 예정 반영 값 포함)·예정 포함 예상 소비(예산 대비 막대), 일별 누적 소비 차트(누르거나 끌면 그날 값, 표로 보기), 카드 실적 현황(목표까지 남은 금액·혜택·한도), 여유자금과 다가오는 카드 결제(오늘 기준), 예정 거래(지난 미확정 + 앞으로 7일, 확정·건너뛰기), 카테고리별 소비(평균·예산 비교) |
| **수동 입력** | 지출 / 수입 / 장부 이체. 금액(지출은 환불·취소), 내용(과거 내용을 고르면 장부·항목·결제수단·혜택을 채움), 날짜, 장부(이체는 보내는·받는 장부), 항목, 결제수단, 할부(신용카드), 카드 혜택·전월실적 제외(카드), 구성원, 메모, 확정/예정 |
| **최근 내역** | 확정 거래를 날짜 최신순으로 30건씩 이어서 불러옵니다(끝 가까이 내리면 다음 쪽). 맨 위 **자동 입력만 보기**(입력 경로가 자동 입력 API인 거래)는 마지막 상태를 기기(SharedPreferences)에 둡니다. 거래를 누르면 원문까지 자세히 보고 **항목·카드 혜택·메모**를 고쳐 저장합니다(항목은 그 거래 장부의 것만, 수입 항목이면 혜택 없음, 장부 이체는 메모만, 저장하지 않고 닫으면 버림) |
| **설정** | 탭 **서버**(주소·토큰·연결 확인) / **권한** / **규칙**(자동 업로드 스위치·규칙 목록) / **업로드 기록**. 서버 미설정·필요한 권한 없음·업로드 실패가 있으면 하단 메뉴와 탭에 표시 |

대시보드·수동 입력·최근 내역은 화면이 다시 보일 때와, 이 앱에서 거래를 넣거나 자동 입력 업로드가 성공했을 때 다시 읽습니다(아래로 당겨 새로 고침도 됨).
토큰 권한을 하나로 합치기 전의 가계부 서버는 예전 '자동 입력 전용' 토큰으로 GraphQL을 막으므로, 그때는 세 화면에 서버를 업데이트하라는 안내와 **서버 설정 열기**가 나옵니다(자동 입력은 그대로 동작).

## 처음 설정

1. **서버 설정**(설정 → 서버): 가계부 주소(`https://…`)와 API 토큰을 넣고 "연결 확인".
   토큰은 가계부 웹 **설정 → API 토큰**에서 발급합니다. 자동 입력과 대시보드·수동 입력·최근 내역에 같은 토큰을 씁니다.
   연결 확인은 빈 요청을 보내 422(금액 없음)가 오는지로 인증을 확인하고, GraphQL 읽기(`appInfo`)도 되는지 보며 거래를 만들지 않습니다.
2. **권한**(설정 → 권한): 필요한 것만 허용합니다.
   | 항목 | 필요한 경우 |
   |---|---|
   | 문자 받기 (`RECEIVE_SMS`) | 문자 규칙 |
   | 문자 읽기 (`READ_SMS`, `RECEIVE_MMS`) | 장문(LMS) 승인 문자, "받은 문자에서 번호 고르기", 샘플 가져오기 |
   | 알림 접근 | 앱 알림 규칙 |
   | 배터리 최적화 제외 | 권장 (제조사 절전으로 알림 서비스가 멈추는 것 방지) |
   | 알림 표시 | 업로드 실패 알림 (선택) |

   Android 13 이상에서 APK를 직접 설치하면 알림 접근·문자 권한이 "제한된 설정"으로 막힐 수 있습니다.
   **앱 정보 → ⋮ → 제한된 설정 허용** 후 다시 허용하세요(adb로 설치하면 해당 없음).
3. **규칙 추가**(설정 → 규칙의 오른쪽 아래 버튼) → 문자 규칙 / 앱 알림 규칙.

## 규칙

설정 → 규칙 탭의 목록이 규칙 목록입니다. 문자 규칙과 앱 알림 규칙으로 나뉘고, 각각 **위에서부터** 맞춰 보아 처음 맞는 규칙을 씁니다
(⋮ → 위로/아래로). 스위치로 규칙마다 켜고 끕니다.

| 항목 | 문자 규칙 | 앱 알림 규칙 |
|---|---|---|
| 대상 | 보내는 번호 여러 개 (비우면 모든 번호). 직접 입력하거나 **받은 문자의 번호 목록**에서 고름 | **설치된 앱 목록**에서 고름 (최근 알림이 온 앱이 위에) |
| 추가 조건 | 본문에 포함할 단어 (쉼표로 여러 개, 하나라도 있으면) | 알림 제목에 포함할 글자 (카카오톡처럼 여러 곳의 알림이 오는 앱) + 포함할 단어 |
| 맞춰 볼 원문 | 문자 본문 (`[Web발신]`은 무시) | `제목\n본문` |

### 형식

- **템플릿** — 샘플을 붙여 넣고 바뀌는 부분을 항목으로 바꿉니다. 칩을 누르면 커서 위치에 들어갑니다.

  | 항목 | 맞는 값 | 서버로 보내는 필드 |
  |---|---|---|
  | `{금액}` | `12,500`, `12500` | `amount` |
  | `{가맹점}` | 한 줄 안의 글자 (마지막 항목이거나 `{*}` 앞이면 줄 끝까지) | `merchant` |
  | `{날짜}` | `10/23`, `10.23`, `2026-10-23`, `26.10.23`, `10월 23일` (연도가 없으면 올해, 일주일 넘게 미래면 작년) | `date` |
  | `{시각}` | `12:34`, `12:34:56` | (보내지 않음, 시험 화면에만 표시) |
  | `{할부}` | `일시불`, `03개월` | `installmentMonths` |
  | `{카드}` | 한 줄 안의 글자 | `card` (카드별 결제수단·규칙의 결제수단·`{끝자리}`가 없을 때) |
  | `{끝자리}` | 숫자 4자리 (`1*2*`처럼 가려진 값은 버림) | `card` (카드별 결제수단·규칙의 결제수단이 없을 때) |
  | `{*}` | 아무 글자나 건너뜀 (줄바꿈 포함) | — |

  항목 밖의 글자는 그대로 맞춰 보고, 공백·줄바꿈은 있어도 없어도 맞습니다. 원문의 일부만 맞아도 됩니다.
  예: `{카드}({끝자리})승인 {*} {금액}원({할부}){날짜} {시각} {가맹점} 누적{*}`
  → `신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원`
- **정규식** — Java 정규식의 이름 있는 그룹 `(?<amount>…)`, `merchant`, `date`, `time`, `installment`, `card`, `last4`.
- **서버 해석** — 원문만 보내고 서버의 카드 승인 문자 해석(`server/src/domain/sms.rs`)을 씁니다. 광고 문자도 금액이 있으면 거래가 되므로 "포함할 단어"(예: `승인`)로 거르세요.

옵션
- **형식이 맞지 않아도 원문 보내기**: 끄면(기본) 대상 번호/앱에서 왔지만 형식이 맞지 않는 메시지는 보내지 않고 기록에 "건너뜀"으로 남깁니다. 기록에서 "원문 보내기"로 따로 보낼 수 있습니다.
- **취소/환불 문자 형식**: 이 규칙으로 보내면 `cancel: true`(음수 금액). 승인 규칙보다 위에 두세요. (서버도 원문에서 "승인취소" 같은 말을 찾아 취소로 봅니다.)
- **서버로 보낼 값**: 결제수단, 장부, 카테고리. 가계부 서버에서 받은 목록(`GET /api/ingest/options`, 보관한 항목 제외)에서 고릅니다.
  비우면 서버가 원문·과거 내역으로 찾습니다. 목록은 규칙 화면을 열 때마다 새로 받고(⟳로 다시 받기), 마지막으로 받은 목록을 기기에 두어
  서버에 연결되지 않을 때 씁니다. 서버 주소가 없거나 목록을 한 번도 받지 못했으면 이름을 직접 입력합니다.
  기본 장부가 아닌 장부의 카테고리를 고르면 장부도 같이 정해지고, 장부를 바꾸면 그 장부에 없는 카테고리는 비워집니다.
- **카드별 결제수단**: 한 규칙으로 같은 카드사의 여러 카드 알림을 받을 때, 형식에서 뽑은 `{카드}`·`{끝자리}` 값에 따라 결제수단을 고릅니다.
  줄마다 "카드 값"과 결제수단을 정하고, 뽑은 값에 카드 값이 들어 있으면(공백·대소문자 무시) 그 결제수단을 보냅니다.
  여러 줄이 맞으면 더 긴 카드 값의 줄을 쓰므로 순서는 상관없습니다(예: `신한` → A, `Deep Dream` → B면 `신한카드 Deep Dream`은 B).
  맞는 줄이 없으면 위의 결제수단(비었으면 `{끝자리}` → `{카드}`)을 씁니다. 시험 샘플에서 뽑힌 값은 "샘플의 '…' 추가"로 바로 줄을 만들 수 있고,
  시험 결과에 어떤 줄로 결제수단이 정해졌는지 보입니다. 서버 해석 방식에는 뽑은 값이 없어 쓰지 않습니다.

규칙 화면 아래 **시험**에 샘플을 넣으면(또는 받은 문자/최근 알림에서 가져오면) 뽑히는 값과 실제로 보낼 JSON을 바로 보여 줍니다.

### 서버로 보내는 내용

```json
{"text": "<받은 원문>", "amount": 12500, "merchant": "스타벅스", "date": "2026-10-23",
 "installmentMonths": 1, "card": "1234", "cancel": true, "ledger": "…", "category": "…"}
```

`text`는 항상 보내고 나머지는 형식에서 뽑힌 값·규칙에 적은 값만 넣습니다. 서버는 명시 필드를 먼저 쓰고 빠진 값은 원문에서 찾으며,
같은 원문은 해시로 중복을 막습니다(`duplicate: true` → 기록에 "중복"). 같은 결제를 문자와 앱 알림 두 규칙으로 함께 받으면 원문이 달라 두 건이 되니 한쪽만 쓰세요.

## 동작

```
SMS_RECEIVED ─────────┐
WAP_PUSH(장문) → MmsWorker(받은편지함에서 본문 읽기) ─┤
NotificationListenerService ──┘
        → RuleMatcher(규칙·형식) → 기록(history.json, 원문 중복 무시) → UploadWorker → POST /api/ingest
```

- **장문(LMS)**: `[Web발신]` 승인 문자는 길면 LMS(MMS)로 와서 SMS 방송이 없습니다. 장문 도착 신호(WAP push)를 받으면
  기본 문자 앱이 내려받아 저장한 본문을 문자 저장소에서 읽습니다(`READ_SMS` 필요). 처음 허용할 때 그 전 메시지는 건너뜁니다.
- **RCS(채팅+)** 메시지는 문자 저장소·방송으로 오지 않습니다. 이 경우 문자 앱(삼성 메시지·Google 메시지)을 **앱 알림 규칙**으로 고르고 제목에 보낸 사람을 넣으세요.
- **업로드**: WorkManager가 네트워크가 있을 때 바로(긴급 작업) 보냅니다. 네트워크·서버 오류(5xx·408·429)는 30초부터 늘려 가며 10번까지 다시 시도하고,
  401·403·404·422 같은 오류는 바로 "실패"로 두고 알림을 띄웁니다. 설정 → 업로드 기록 탭에서 결과(입력된 금액·카테고리·결제수단·경고)와 원문·보낸 JSON을 보고 다시 보낼 수 있습니다.
- **기록**은 최근 300건(건너뜀은 30건)만 기기에 둡니다. 최근 알림 목록(샘플용)은 메모리에만 두고 앱이 다시 시작되면 지워집니다.
- **보안**: 기본은 HTTPS만 허용합니다(토큰 평문 전송 방지). 예외는 `localhost`·`127.0.0.1`(adb reverse)과 에뮬레이터의 `10.0.2.2`뿐입니다(`res/xml/network_security_config.xml`).
  토큰·기록은 앱 저장소에만 두고 백업·기기 이전에서 제외합니다. 요청에 `Origin`을 보내지 않으므로 서버의 교차 사이트 차단에 걸리지 않습니다.

## 코드 구조

```
app/src/main/java/com/rouddy/cashbook/relay/
  RelayApp.kt              Application, Graph(Store·코루틴 범위)
  MainActivity.kt
  data/Models.kt, Store.kt 규칙·설정(config.json), 업로드 기록(history.json) — DataStore + JSON
  parse/MessageFormat.kt   템플릿/정규식 → 값 추출, 날짜·할부 해석
  parse/PhoneNumber.kt     발신 번호 정리(+82, 대표번호)
  ingest/RuleMatcher.kt    규칙 맞추기, 요청 JSON 만들기, 시험
  ingest/Ingestor.kt       받은 메시지 처리·다시 보내기
  ingest/UploadWorker.kt   서버 전송·재시도
  ingest/CashbookApi.kt    /api/ingest, 연결 확인, 응답 해석
  source/                  SmsReceiver, MmsReceiver + MmsWorker, MessageReader(문자 저장소), NotificationCaptureService
  data/UiPrefs.kt          화면 선택 상태(SharedPreferences): 최근 내역 "자동 입력만", 대시보드 장부
  book/                    가계부 GraphQL: Queries(문서), BookModels(응답), BookApi(해석·오류 종류·BookRepository),
                           EntryForm(수동 입력 규칙·요청 JSON), TxEdit(최근 내역 고치기 — 바뀐 값만 보내는 patch), BookFormat(금액·날짜·이체 표시)
  ui/                      AppNav(하단 메뉴), DashboardScreen·DailyChart·BookUi(막대·차트 색), EntryScreen, RecentScreen,
                           SettingsScreen(탭: 서버) + PermissionsPane·RulesPane·HistoryPane, 규칙 편집, 앱/번호/샘플 선택
```

## 에뮬레이터로 시험

```bash
# 1) 테스트용 서버 (실제 DB 사본으로, 루트 README의 E2E와 같은 방식)
sqlite3 server/data/cashbook.db ".backup /tmp/cashbook-android-e2e.db"
(cd server && DATABASE_URL="sqlite:///tmp/cashbook-android-e2e.db?mode=rwc" CASHBOOK_PASSWORD=e2e-pass \
   CASHBOOK_SECRET_FILE=/tmp/cashbook-android-e2e.key ROCKET_PORT=8001 cargo run) &

# 2) 앱 설치, 에뮬레이터의 127.0.0.1:8001을 PC로 연결 → 앱 서버 주소 http://127.0.0.1:8001
#    (installDebug는 연결된 모든 기기에 설치하므로 실제 휴대폰이 연결돼 있으면 adb -s로 에뮬레이터에만 설치)
cd android && ./gradlew assembleDebug && adb -s emulator-5554 install -r app/build/outputs/apk/debug/cashbook_*.apk
adb -s emulator-5554 reverse tcp:8001 tcp:8001
#    토큰은 사본 서버 웹(http://127.0.0.1:8001) 설정 → API 토큰에서 발급해 넣는다

# 3) 승인 문자 보내기 (문자 규칙의 번호를 15447200으로)
adb emu sms send 15447200 "신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원"

# 4) 알림 보내기 (앱 알림 규칙의 앱을 com.android.shell로)
adb shell cmd notification post -t "카카오페이" test-tag "스타벅스 4,500원 결제 완료"
```

권한은 adb로도 줄 수 있습니다: `adb shell pm grant com.rouddy.cashbook.relay android.permission.RECEIVE_SMS`(READ_SMS·RECEIVE_MMS 동일),
`adb shell cmd notification allow_listener com.rouddy.cashbook.relay/.source.NotificationCaptureService`.
