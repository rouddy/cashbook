// E2E 스모크 테스트: 실제 브라우저(시스템 Chrome)로 주요 화면과 입력 흐름을 확인한다.
//
// 테스트용 DB로 띄운 서버가 필요하다 (실제 데이터를 쓰지 말 것). 로그인은 이 스크립트가 띄우는 가짜 Google 서버(8002)로 한다:
//   sqlite3 ../server/data/cashbook.db ".backup /tmp/cashbook-e2e.db"
//   (cd ../server && DATABASE_URL="sqlite:///tmp/cashbook-e2e.db?mode=rwc" CASHBOOK_SECRET_FILE=/tmp/cashbook-e2e.key ROCKET_PORT=8001 \
//      CASHBOOK_GOOGLE_CLIENT_ID=e2e CASHBOOK_GOOGLE_CLIENT_SECRET=e2e CASHBOOK_PUBLIC_URL=http://127.0.0.1:8001 \
//      CASHBOOK_OWNER_EMAIL=owner@e2e.test CASHBOOK_GOOGLE_AUTH_URL=http://127.0.0.1:8002/auth \
//      CASHBOOK_GOOGLE_TOKEN_URL=http://127.0.0.1:8002/token CASHBOOK_GOOGLE_USERINFO_URL=http://127.0.0.1:8002/userinfo cargo run)
//   npm run e2e
// 소유자 이메일로 로그인하므로 DB 사본의 기존 데이터(계정 id 1)를 넘겨받아 쓴다.
//
// 환경 변수: E2E_BASE_URL(기본 http://127.0.0.1:8001), E2E_GOOGLE_PORT(기본 8002), E2E_EMAIL(기본 owner@e2e.test), CHROME_PATH

import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { chromium } from 'playwright-core'

const BASE = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:8001'
const GOOGLE_PORT = Number(process.env.E2E_GOOGLE_PORT ?? 8002)
const EMAIL = process.env.E2E_EMAIL ?? 'owner@e2e.test'
const CHROME = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const STAMP = Date.now().toString(36)

// 가짜 Google: 로그인 화면 없이 바로 코드를 붙여 돌려보내고, 토큰·사용자 정보를 돌려준다
const google = createServer((req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${GOOGLE_PORT}`)
  const json = (body) => {
    res.writeHead(200, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify(body))
  }
  if (url.pathname === '/auth') {
    const back = new URL(url.searchParams.get('redirect_uri'))
    back.searchParams.set('code', 'e2e-code')
    back.searchParams.set('state', url.searchParams.get('state'))
    res.writeHead(302, { Location: back.toString() })
    res.end()
  } else if (url.pathname === '/token') {
    json({ access_token: 'e2e-access', token_type: 'Bearer', expires_in: 3600 })
  } else if (url.pathname === '/userinfo') {
    json({ sub: 'e2e-owner', email: EMAIL, email_verified: true, name: 'E2E 사용자' })
  } else {
    res.writeHead(404)
    res.end()
  }
})
await new Promise((resolve) => google.listen(GOOGLE_PORT, '127.0.0.1', resolve))

/** 카드 상품(혜택·구간 등)을 만들고 그 상품을 고른 카드를 만든다 */
async function createCard(card, product) {
  let cardProductId = null
  if (product) {
    const p = await gql(`mutation($input: CardProductInput!) { createCardProduct(input: $input) { id } }`, {
      input: { name: `${card.name} 상품`, ...product },
    })
    assert.ok(p.data, JSON.stringify(p.errors))
    cardProductId = p.data.createCardProduct.id
  }
  const created = await gql(`mutation($input: PaymentMethodInput!) { createPaymentMethod(input: $input) { id benefits { id name } } }`, {
    input: { ...card, cardProductId },
  })
  assert.ok(created.data, JSON.stringify(created.errors))
  return { ...created.data.createPaymentMethod, cardProductId }
}

const browser = await chromium.launch({ executablePath: CHROME, headless: true })
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, locale: 'ko-KR', timezoneId: 'Asia/Seoul' })
const page = await context.newPage()
const pageErrors = []
page.on('pageerror', (e) => pageErrors.push(e.message))
page.on('console', (m) => {
  // 로그인 실패/로그아웃 뒤의 401은 의도한 응답이다
  if (m.type() === 'error' && !/401|Unauthorized/.test(m.text())) pageErrors.push(m.text())
})

let failures = 0
async function step(name, fn) {
  const started = Date.now()
  try {
    await fn()
    console.log(`✓ ${name} (${Date.now() - started}ms)`)
  } catch (e) {
    failures++
    console.log(`✗ ${name}\n  ${e.message.split('\n').slice(0, 6).join('\n  ')}`)
    await page.screenshot({ path: `/tmp/e2e-fail-${name.replace(/[\s/\\:*?"<>|()·→.]+/g, '_')}.png`, fullPage: true }).catch(() => {})
  }
}

/** 브라우저 세션(쿠키)으로 GraphQL 호출 */
async function gql(query, variables) {
  return page.evaluate(
    async ([q, v]) => {
      const res = await fetch('/api/graphql', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query: q, variables: v }),
      })
      return res.json()
    },
    [query, variables ?? {}],
  )
}

/** 조건이 참이 될 때까지 기다린다 (저장 뒤 서버 값 확인용) */
async function waitUntil(fn, label, timeout = 5000) {
  const until = Date.now() + timeout
  for (;;) {
    if (await fn()) return
    if (Date.now() > until) throw new Error(`시간 초과: ${label}`)
    await new Promise((r) => setTimeout(r, 100))
  }
}

function acceptNextDialog() {
  page.once('dialog', (d) => d.accept())
}

await step('Google 로그인 (맞지 않는 요청 → 이유 표시, 로그인 → 보던 화면, 기존 데이터 계정)', async () => {
  await page.goto(`${BASE}/api/auth/google/callback?code=x&state=wrong`)
  await page.getByText('로그인 요청이 만료됐거나 맞지 않습니다').waitFor()
  await page.goto(`${BASE}/plan`)
  await page.getByRole('link', { name: 'Google로 로그인' }).click()
  await page.getByRole('heading', { name: '지출 계획' }).waitFor()
  const session = await page.evaluate(async () => (await fetch('/api/session')).json())
  assert.equal(session.user.id, 1, '소유자 이메일은 기존 데이터 계정을 넘겨받는다')
  assert.equal(session.user.isAdmin, true)
  await page.goto(`${BASE}/`)
  await page.getByRole('heading', { name: '대시보드' }).waitFor()
  await page.getByText('E2E 사용자').first().waitFor()
})

await step('대시보드: 지표·여유자금·차트·예정 거래', async () => {
  await page.goto(`${BASE}/?month=2026-08`)
  await page.getByText('2026년 8월 소비').waitFor()
  await page.getByText('5,349,932원').first().waitFor()
  await page.getByRole('heading', { name: '여유자금' }).waitFor()
  await page.getByRole('img', { name: /일별 누적 소비/ }).waitFor()
  await page.getByRole('heading', { name: '예정 거래' }).waitFor()
  // 차트 툴팁: 키보드로 크로스헤어 이동
  const chart = page.getByRole('img', { name: /일별 누적 소비/ })
  await chart.focus()
  await page.keyboard.press('ArrowRight')
  await page.locator('.chart-tooltip').first().waitFor()
})

await step('가계부: 금액 셀 직접 수정 후 되돌리기', async () => {
  await page.goto(`${BASE}/transactions?month=2026-08`)
  const row = page.locator('table.sheet tbody tr', { hasText: 'Ponz' })
  await row.locator('button.cell-btn').nth(1).click()
  const input = row.locator('input.cell-input')
  await input.fill('11500')
  await input.press('Enter')
  await row.getByText('11,500').waitFor()
  const r = await gql(`{ transactions(filter: {month: "2026-08", search: "Ponz"}) { items { id amount } } }`)
  const tx = r.data.transactions.items[0]
  assert.equal(tx.amount, 11500)
  await gql(`mutation($id: Int!) { updateTransaction(id: $id, input: {amount: 10500}) { id } }`, { id: tx.id })
})

await step('가계부: 항목 셀 select 변경', async () => {
  await page.goto(`${BASE}/transactions?month=2026-08`)
  const row = page.locator('table.sheet tbody tr', { hasText: '봄살롱' })
  await row.locator('button.cell-btn').nth(2).click()
  await row.locator('select.cell-input').selectOption({ label: '외식' })
  await row.getByText('외식').waitFor()
  await row.locator('button.cell-btn').nth(2).click()
  await row.locator('select.cell-input').selectOption({ label: '기타' })
  await row.locator('button.cell-btn', { hasText: '기타' }).waitFor()
})

await step('가계부: 새 행으로 입력 (가맹점 자동완성) 후 선택 → 삭제 확인 창', async () => {
  await page.goto(`${BASE}/transactions?month=2026-10`)
  const newRow = page.locator('tr.new-row')
  await newRow.getByLabel('금액').fill('4500')
  const desc = newRow.getByLabel('내용')
  await desc.fill('스타벅')
  await page.getByRole('option', { name: /스타벅스/ }).first().click()
  // 자동완성이 항목/결제수단을 채운다
  assert.notEqual(await newRow.getByLabel('항목').inputValue(), '')
  await desc.fill(`E2E 커피 ${STAMP}`)
  await newRow.getByRole('button', { name: '추가' }).click()
  const created = page.locator('table.sheet tbody tr', { hasText: `E2E 커피 ${STAMP}` })
  await created.waitFor()
  // 행마다 삭제 버튼은 없고, 선택 모드에서 체크한 뒤 확인 창을 거쳐야 지운다
  assert.equal(await created.getByRole('button', { name: '삭제' }).count(), 0)
  await page.getByRole('button', { name: '선택', exact: true }).click()
  await created.getByRole('checkbox', { name: /선택$/ }).check()
  await page.getByRole('button', { name: '삭제', exact: true }).click()
  const confirm = page.getByRole('dialog')
  await confirm.getByText(`E2E 커피 ${STAMP}`).waitFor()
  await confirm.getByRole('button', { name: '취소' }).click()
  await confirm.waitFor({ state: 'detached' })
  assert.equal(await created.count(), 1)
  await page.getByRole('button', { name: '삭제', exact: true }).click()
  await confirm.getByRole('button', { name: '1건 삭제' }).click()
  await created.waitFor({ state: 'detached' })
})

await step('가계부: 지난 예정 항목 확정 → 다시 예정으로', async () => {
  await page.goto(`${BASE}/transactions?month=2026-10&status=PLANNED`)
  const row = page.locator('table.sheet tbody tr', { hasText: '원리금상환' })
  await row.getByRole('button', { name: '확정' }).click()
  await row.waitFor({ state: 'detached' })
  const r = await gql(`{ transactions(filter: {month: "2026-10", search: "원리금상환"}) { items { id status } } }`)
  assert.equal(r.data.transactions.items[0].status, 'CONFIRMED')
  await gql(`mutation($id: Int!) { updateTransaction(id: $id, input: {status: PLANNED}) { id } }`, { id: r.data.transactions.items[0].id })
})

await step('가계부: 장부 탭 전환과 필터 지우기 (여러 URL 파라미터 동시 변경)', async () => {
  await page.goto(`${BASE}/transactions?month=2026-10`)
  await page.getByRole('tab', { name: '용돈' }).click()
  await page.getByRole('tab', { name: '용돈', selected: true }).waitFor()
  assert.notEqual(new URL(page.url()).searchParams.get('ledger'), null)
  await page.goto(`${BASE}/transactions?month=2026-10&category=none&pm=3&status=PLANNED`)
  await page.getByRole('button', { name: '필터 지우기' }).click()
  await page.waitForFunction(() => {
    const q = new URLSearchParams(location.search)
    return !q.has('category') && !q.has('pm') && !q.has('status')
  })
})

await step('가계부: 셀 편집 중 다른 입력칸을 누르면 그 칸에 입력된다', async () => {
  await page.goto(`${BASE}/transactions?month=2026-08`)
  const row = page.locator('table.sheet tbody tr', { hasText: 'Ponz' })
  await row.locator('button.cell-btn').nth(1).click()
  await row.locator('input.cell-input').waitFor()
  const newAmount = page.locator('tr.new-row').getByLabel('금액')
  await newAmount.click()
  await page.waitForTimeout(150)
  await page.keyboard.type('77')
  assert.equal(await newAmount.inputValue(), '77')
  await newAmount.fill('')
})

await step('가계부: 새 행에서 Enter를 연달아 눌러도 한 건만 저장', async () => {
  await page.goto(`${BASE}/transactions?month=2026-10`)
  const newRow = page.locator('tr.new-row')
  await newRow.getByLabel('금액').fill('1234')
  await newRow.getByLabel('내용').fill(`E2E 연타 ${STAMP}`)
  await newRow.getByLabel('메모').click()
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  await page.locator('table.sheet tbody tr', { hasText: `E2E 연타 ${STAMP}` }).first().waitFor()
  await page.waitForTimeout(800)
  const r = await gql(`{ transactions(filter: {search: "E2E 연타 ${STAMP}"}) { totalCount items { id } } }`)
  assert.equal(r.data.transactions.totalCount, 1)
  await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: r.data.transactions.items[0].id })
})

await step('결제 입력: 폼 저장 + 문자 해석', async () => {
  await page.goto(`${BASE}/entry`)
  await page.getByLabel('금액').fill('23000')
  await page.getByLabel('내용').fill(`E2E 점심 ${STAMP}`)
  await page.getByRole('button', { name: '외식', exact: true }).click()
  await page.getByLabel('결제수단').selectOption({ label: 'MG+S' })
  await page.getByRole('button', { name: '저장', exact: true }).click()
  await page.getByText(`E2E 점심 ${STAMP} 23,000원 저장했습니다`).waitFor()
  await page.getByPlaceholder(/Web발신/).fill('[Web발신]\n하나카드(5678)승인 홍*동님 8,900원 일시불 10/23 08:15 GS25역삼점 누적 300,000원')
  await page.getByRole('button', { name: '해석해서 왼쪽에 채우기' }).click()
  await page.locator('dl.kv').getByText('GS25역삼점').waitFor()
  assert.equal(await page.getByLabel('금액').inputValue(), '8900')
  const r = await gql(`{ transactions(filter: {search: "E2E 점심 ${STAMP}"}) { items { id amount categoryId paymentMethodId } } }`)
  assert.equal(r.data.transactions.items.length, 1)
  await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: r.data.transactions.items[0].id })
})

let newCardId = null
await step('카드 추가 (정산 일정 자동 추정·혜택·구간) → 상세 → 삭제', async () => {
  await page.goto(`${BASE}/cards`)
  await page.getByRole('button', { name: '+ 카드 추가' }).click()
  const modal = page.getByRole('dialog')
  await modal.getByLabel('이름 *').fill(`E2E카드${STAMP}`)
  await modal.getByLabel('카드사').fill('신한')
  await modal.getByLabel('결제일').selectOption('25')
  await modal.getByRole('button', { name: '결제일로 자동 추정' }).click()
  await modal.getByText('이용기간 전월 12일 ~ 당월 11일').waitFor()
  // 카드 상품은 카드 폼 안에서 새로 만들고, 만들면 그 상품이 골라진 채로 카드 폼으로 돌아온다
  await modal.getByRole('button', { name: '새 상품' }).click()
  assert.equal(await modal.getByLabel('카드사').inputValue(), '신한')
  await modal.getByLabel('상품명 *').fill(`E2E상품${STAMP}`)
  await modal.getByLabel('전월실적 목표').fill('300000')
  await modal.getByRole('button', { name: '+ 혜택 추가' }).click()
  const benefitRow = modal.locator('tbody tr').last()
  await benefitRow.locator('input').nth(0).fill('커피')
  await benefitRow.locator('input').nth(1).fill('10')
  await modal.getByRole('button', { name: '+ 구간 추가' }).click()
  await modal.getByLabel('전월실적 이상').fill('300000')
  await modal.getByLabel('혜택 한도').fill('10000')
  await modal.getByRole('button', { name: '상품 추가' }).click()
  await modal.getByText('혜택').filter({ hasText: '혜택' }).first().waitFor()
  assert.equal(await modal.getByLabel('이름 *').inputValue(), `E2E카드${STAMP}`, '카드 폼 입력은 그대로')
  assert.match(await modal.getByRole('combobox', { name: '카드 상품' }).locator('option:checked').innerText(), new RegExp(`E2E상품${STAMP}`))
  await modal.getByRole('button', { name: '추가', exact: true }).click()
  await page.getByRole('heading', { name: `E2E카드${STAMP}`, exact: true }).waitFor()
  await page.getByText('매월 25일 결제 · 전월 12일 ~ 당월 11일 이용').waitFor()
  await page.getByRole('heading', { name: '결제 예정 금액' }).waitFor()
  newCardId = Number(page.url().match(/cards\/(\d+)/)[1])
  const r = await gql(`query($id: Int!) { paymentMethod(id: $id) { benefits { name rate } tiers { minSpending benefitLimit } closingDay closingMonthOffset } }`, { id: newCardId })
  assert.deepEqual(r.data.paymentMethod.benefits, [{ name: '커피', rate: 0.1 }])
  assert.deepEqual(r.data.paymentMethod.tiers, [{ minSpending: 300000, benefitLimit: 10000 }])
  assert.equal(r.data.paymentMethod.closingDay, 11)
  acceptNextDialog()
  await page.getByRole('button', { name: '삭제' }).click()
  await page.getByRole('heading', { name: '카드', exact: true }).waitFor()
  const after = await gql(`query($id: Int!) { paymentMethod(id: $id) { id } }`, { id: newCardId })
  assert.equal(after.data.paymentMethod, null)
  // 카드 상품 목록에서 쓰는 카드가 없어진 상품을 지운다
  const productRow = page.locator('tbody tr', { hasText: `E2E상품${STAMP}` })
  acceptNextDialog()
  await productRow.getByRole('button', { name: '삭제' }).click()
  await productRow.waitFor({ state: 'detached' })
})

await step('카드 상세: 기존 카드 정산 일정 입력 → 결제 예정 표시', async () => {
  await page.goto(`${BASE}/cards/2?edit=1&month=2026-08`)
  const form = page.locator('form').first()
  await form.getByLabel('결제일').selectOption('14')
  await form.getByRole('button', { name: '결제일로 자동 추정' }).click()
  await form.getByText('이용기간 전월 1일 ~ 전월 말일').waitFor()
  await form.getByRole('button', { name: '저장' }).click()
  await page.getByText('매월 14일 결제 · 전월 1일 ~ 전월 말일 이용').waitFor()
  await page.getByText('다음 결제').first().waitFor()
  await page.getByRole('heading', { name: /2026년 8월 실적/ }).waitFor()
})

await step('카드별 내역: 혜택 태그 편집과 결제월 기준 보기', async () => {
  await page.goto(`${BASE}/card-transactions?month=2026-08&card=3`)
  await page.getByRole('heading', { name: /2026년 8월 이용 내역/ }).waitFor()
  const row = page.locator('table.sheet tbody tr', { hasText: 'GS25' }).first()
  // 날짜·금액·내용·장부·항목·혜택 순서라 혜택 칸은 6번째
  await row.locator('button.cell-btn').nth(5).click()
  await row.getByRole('checkbox', { name: /간편결제/ }).check()
  await row.getByRole('button', { name: '적용' }).click()
  await row.locator('button.cell-btn', { hasText: '간편결제' }).waitFor()
  // 정산 일정이 없는 카드는 결제월 기준으로 묶을 수 없다는 안내
  await page.getByRole('button', { name: '결제월 기준' }).click()
  await page.getByText('정산 일정이 없어 결제월 기준으로 묶을 수 없습니다').waitFor()
  // 앞 단계에서 정산 일정을 넣은 탄탄대로biz(14일 결제, 전월 1일~말일)의 9월 결제분 = 8월 이용분
  await page.goto(`${BASE}/card-transactions?month=2026-09&card=2&mode=statement`)
  await page.getByRole('heading', { name: '2026년 9월 결제분' }).waitFor()
  const footer = page.locator('tfoot', { hasText: '결제 금액' })
  await footer.waitFor()
  const r = await gql(`{ statementLines(paymentMethodId: 2, month: "2026-09") { amount } }`)
  const total = r.data.statementLines.reduce((a, l) => a + l.amount, 0)
  await footer.getByText(total.toLocaleString('ko-KR')).waitFor()
})

await step('카드별 내역: 내용 자동완성은 최근 3개월 안, 같은 내용이라도 항목이 다르면 따로', async () => {
  const ref = await gql(`{ ledgers { id isDefault } categories { id name ledgerId kind } }`)
  const home = ref.data.ledgers.find((l) => l.isDefault) ?? ref.data.ledgers[0]
  const cats = ref.data.categories.filter((c) => c.ledgerId === home.id && c.kind === 'EXPENSE')
  assert.ok(cats.length >= 3, '기본 장부에 지출 항목이 3개 이상 필요합니다')
  const name = `E2E가맹점${STAMP}`
  const ids = []
  // 8월에 넣으면 기준일은 8/1 → 5/1 이후 내역만. 2월 것(세 번째 항목)은 나오지 않아야 한다
  for (const [date, category] of [
    ['2026-07-10', cats[0]],
    ['2026-07-20', cats[1]],
    ['2026-02-10', cats[2]],
  ]) {
    const r = await gql(`mutation($input: TransactionInput!) { createTransaction(input: $input) { id } }`, {
      input: { ledgerId: home.id, date, amount: 1000, categoryId: category.id, paymentMethodId: 3, description: name },
    })
    ids.push(r.data.createTransaction.id)
  }
  try {
    await page.goto(`${BASE}/card-transactions?month=2026-08&card=3`)
    const newRow = page.locator('tr.new-row')
    await newRow.getByLabel('내용').fill(name)
    const options = page.getByRole('option', { name: new RegExp(name) })
    await options.first().waitFor()
    assert.equal(await options.count(), 2)
    // 횟수가 같으면 최근 것부터, 항목 이름을 붙여 따로 보인다
    assert.match(await options.nth(0).locator('.badge').innerText(), new RegExp(`${cats[1].name}$`))
    assert.match(await options.nth(1).locator('.badge').innerText(), new RegExp(`${cats[0].name}$`))
    await options.nth(1).click()
    assert.equal(await newRow.getByLabel('항목').inputValue(), String(cats[0].id))
  } finally {
    for (const id of ids) await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id })
  }
})

await step('카드 혜택: 중복 적용·한도 없는 혜택·혜택 종류', async () => {
  const card = await createCard(
    { name: `E2E혜택${STAMP}`, kind: 'CREDIT_CARD' },
    {
      benefits: [
        { name: '백화점', rate: 0.01 },
        { name: '할인점', rate: 0.01 },
        { name: '간편결제', rate: 0.03, stackable: true },
        { name: '기본적립', kind: 'POINT', rate: 0.001, sharesCardLimit: false },
      ],
      tiers: [{ minSpending: 0, benefitLimit: 3000 }],
    },
  )
  const id = Object.fromEntries(card.benefits.map((b) => [b.name, b.id]))
  const tx = await gql(`mutation($input: TransactionInput!) { createTransaction(input: $input) { id } }`, {
    input: { date: '2026-08-10', amount: 100000, paymentMethodId: card.id, description: `E2E혜택거래${STAMP}` },
  })
  const txId = tx.data.createTransaction.id
  await page.goto(`${BASE}/cards/${card.id}?month=2026-08`)
  const row = page.locator('table.sheet tbody tr', { hasText: `E2E혜택거래${STAMP}` })
  await row.locator('button.cell-btn').nth(4).click()
  // 중복 적용이 아닌 혜택은 하나만: 할인점을 고르면 백화점이 풀린다
  await row.getByRole('checkbox', { name: /백화점/ }).check()
  await row.getByRole('checkbox', { name: /할인점/ }).check()
  assert.equal(await row.getByRole('checkbox', { name: /백화점/ }).isChecked(), false)
  await row.getByRole('checkbox', { name: /간편결제/ }).check()
  await row.getByRole('button', { name: '적용' }).click()
  await row.locator('button.cell-btn', { hasText: '할인점, 간편결제' }).waitFor()
  // 1% + 3% = 4,000원이지만 통합 한도 3,000원
  await row.getByText('3,000', { exact: true }).waitFor()
  const bad = await gql(`mutation($id: Int!, $ids: [Int!]) { updateTransaction(id: $id, input: { benefitIds: $ids }) { id } }`, {
    id: txId,
    ids: [id['백화점'], id['할인점']],
  })
  assert.match(bad.errors[0].message, /함께 받을 수 없습니다/)
  // 한도 없는 포인트 적립 + 간편결제: 한도 3,000원을 다 써도 포인트 100P는 따로 받는다
  await gql(`mutation($id: Int!, $ids: [Int!]) { updateTransaction(id: $id, input: { benefitIds: $ids }) { id } }`, {
    id: txId,
    ids: [id['기본적립'], id['간편결제']],
  })
  const report = await gql(`query($id: Int!) { paymentMethod(id: $id) { monthReport(month: "2026-08") { discountTotal pointTotal limitUsed } } }`, {
    id: card.id,
  })
  assert.deepEqual(report.data.paymentMethod.monthReport, { discountTotal: 3000, pointTotal: 100, limitUsed: 3000 })
  await page.reload()
  await page.getByText('3,000원 · 100P').first().waitFor()
  await page.locator('tr', { hasText: '기본적립' }).getByText('한도 없음').waitFor()
  await page.locator('tr', { hasText: '간편결제' }).getByText('중복 적용').first().waitFor()
  // 혜택은 카드 상품에 있다: 카드 편집의 '상품 수정'에서 고친다.
  // 종류를 마일리지로 바꾸면 혜택률을 "N원당 1마일"로 다시 적는다 (0.1% → 1,000원당 1마일)
  await page.goto(`${BASE}/cards/${card.id}?edit=1&month=2026-08`)
  await page.locator('form').first().getByRole('button', { name: '상품 수정' }).click()
  const form = page.locator('form').first()
  const pointRow = form.locator('tbody tr').nth(3)
  await pointRow.getByLabel('혜택 종류').selectOption('MILEAGE')
  assert.equal(await pointRow.getByLabel('1마일당 이용금액').inputValue(), '1000')
  await form.getByRole('button', { name: '상품 저장' }).click()
  await page.getByRole('button', { name: '상품 수정' }).waitFor()
  await page.locator('tr', { hasText: '기본적립' }).getByText('1,000원당 1마일').waitFor()
  const saved = await gql(`query($id: Int!) { paymentMethod(id: $id) { benefits { name kind rate stackable } } }`, { id: card.id })
  assert.deepEqual(saved.data.paymentMethod.benefits[3], { name: '기본적립', kind: 'MILEAGE', rate: 0.001, stackable: false })
  await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: txId })
  const del = await gql(`mutation($id: Int!) { deletePaymentMethod(id: $id) { deleted } }`, { id: card.id })
  assert.equal(del.data.deletePaymentMethod.deleted, true)
  const delProduct = await gql(`mutation($id: Int!) { deleteCardProduct(id: $id) }`, { id: card.cardProductId })
  assert.equal(delProduct.data.deleteCardProduct, true)
})

await step('가계부: 확정 거래의 상태 칸 혜택 버튼으로 카드 혜택 고르기', async () => {
  const card = await createCard(
    { name: `E2E상태칸카드${STAMP}`, kind: 'CREDIT_CARD' },
    {
      benefits: [
        { name: '백화점', rate: 0.01 },
        { name: '간편결제', rate: 0.03, stackable: true },
      ],
    },
  )
  const id = Object.fromEntries(card.benefits.map((b) => [b.name, b.id]))
  const ids = []
  for (const status of ['CONFIRMED', 'PLANNED']) {
    const r = await gql(`mutation($input: TransactionInput!) { createTransaction(input: $input) { id } }`, {
      input: { date: '2026-08-12', amount: 20000, paymentMethodId: card.id, description: `E2E상태칸${status}${STAMP}`, status },
    })
    ids.push(r.data.createTransaction.id)
  }
  try {
    await page.goto(`${BASE}/transactions?month=2026-08&pm=${card.id}`)
    const confirmed = page.locator('table.sheet tbody tr', { hasText: `E2E상태칸CONFIRMED${STAMP}` })
    const planned = page.locator('table.sheet tbody tr', { hasText: `E2E상태칸PLANNED${STAMP}` })
    await planned.waitFor()
    // 예정 거래에는 혜택 버튼이 없다
    assert.equal(await planned.getByRole('button', { name: /혜택/ }).count(), 0)
    await confirmed.getByRole('button', { name: /혜택 고르기/ }).click()
    // 창은 표 스크롤 영역에 잘리지 않게 body에 띄운다
    const dialog = page.getByRole('dialog', { name: '혜택 고르기' })
    await dialog.getByRole('checkbox', { name: /백화점/ }).check()
    await dialog.getByRole('checkbox', { name: /간편결제/ }).check()
    await dialog.getByRole('button', { name: '적용' }).click()
    await confirmed.getByRole('button', { name: /혜택: 백화점, 간편결제/ }).waitFor()
    // 화면은 저장 전에 먼저 바뀌므로(낙관적 반영) 서버 값은 저장이 끝날 때까지 기다린다
    const want = JSON.stringify([id['백화점'], id['간편결제']].sort((a, b) => a - b))
    await waitUntil(async () => {
      const saved = await gql(`query($id: Int!) { transaction(id: $id) { benefitIds } }`, { id: ids[0] })
      return JSON.stringify([...saved.data.transaction.benefitIds].sort((a, b) => a - b)) === want
    }, '혜택 저장')
    // 다시 열어 하나를 풀고 Esc로 닫으면 저장하지 않고 버튼으로 포커스가 돌아온다
    const button = confirmed.getByRole('button', { name: /혜택:/ })
    await button.click()
    await dialog.getByRole('checkbox', { name: /백화점/ }).uncheck()
    await page.keyboard.press('Escape')
    await dialog.waitFor({ state: 'detached' })
    assert.equal(await button.evaluate((el) => el === document.activeElement), true)
    // 바깥을 눌러도 저장하지 않고 닫는다
    await button.click()
    await dialog.getByRole('checkbox', { name: /백화점/ }).uncheck()
    await page.getByRole('heading', { name: '가계부' }).click()
    await dialog.waitFor({ state: 'detached' })
    await confirmed.getByRole('button', { name: /혜택: 백화점, 간편결제/ }).waitFor()
  } finally {
    for (const txId of ids) await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: txId })
    await gql(`mutation($id: Int!) { deletePaymentMethod(id: $id) { deleted } }`, { id: card.id })
  }
})

await step('가계부·카드별 내역: 실적제외 체크박스로 전월실적에서 빼기', async () => {
  const created = await gql(`mutation($input: PaymentMethodInput!) { createPaymentMethod(input: $input) { id } }`, {
    input: { name: `E2E실적카드${STAMP}`, kind: 'CHECK_CARD' },
  })
  const card = created.data.createPaymentMethod
  const performance = async () =>
    (await gql(`query($id: Int!) { paymentMethod(id: $id) { monthReport(month: "2026-08") { performance } } }`, { id: card.id })).data.paymentMethod
      .monthReport.performance
  await gql(`mutation($input: TransactionInput!) { createTransaction(input: $input) { id } }`, {
    input: { date: '2026-08-12', amount: 30000, paymentMethodId: card.id, description: `E2E실적기존${STAMP}` },
  })
  try {
    assert.equal(await performance(), 30000)
    // 가계부: 기존 행을 체크하면 바로 저장되어 실적에서 빠진다
    await page.goto(`${BASE}/transactions?month=2026-08&pm=${card.id}`)
    const listed = page.locator('table.sheet tbody tr', { hasText: `E2E실적기존${STAMP}` })
    await listed.getByRole('checkbox', { name: /전월실적 제외/ }).check()
    await waitUntil(async () => (await performance()) === 0, '실적 제외 반영')
    await page.reload()
    assert.equal(await listed.getByRole('checkbox', { name: /전월실적 제외/ }).isChecked(), true)

    // 카드별 내역: 새 행에서 체크하고 추가하면 실적 제외로 저장되고, 새 행 체크는 풀린다
    await page.goto(`${BASE}/card-transactions?month=2026-08&card=${card.id}`)
    const newRow = page.locator('tr.new-row')
    const newCheck = newRow.getByRole('checkbox', { name: '전월실적 제외' })
    await newRow.getByLabel('금액').fill('12000')
    await newRow.getByLabel('내용').fill(`E2E실적새행${STAMP}`)
    await newCheck.check()
    await newRow.getByRole('button', { name: '추가' }).click()
    const added = page.locator('table.sheet tbody tr', { hasText: `E2E실적새행${STAMP}` })
    await added.waitFor()
    assert.equal(await added.getByRole('checkbox', { name: /전월실적 제외/ }).isChecked(), true)
    // 새 행은 저장 결과를 다시 읽은 뒤에 비운다
    await waitUntil(async () => !(await newCheck.isChecked()), '새 행 체크 초기화')
    assert.equal(await performance(), 0)
    // 기존 행을 풀면 실적에 다시 들어간다
    await page.locator('table.sheet tbody tr', { hasText: `E2E실적기존${STAMP}` }).getByRole('checkbox', { name: /전월실적 제외/ }).uncheck()
    await waitUntil(async () => (await performance()) === 30000, '실적 제외 해제 반영')

    // 가계부 새 행: 카드 결제수단을 골라야 체크할 수 있다
    await page.goto(`${BASE}/transactions?month=2026-08`)
    const ledgerRow = page.locator('tr.new-row')
    assert.equal(await ledgerRow.getByRole('checkbox', { name: '전월실적 제외' }).isDisabled(), true)
    await ledgerRow.getByLabel('결제수단').selectOption({ label: `E2E실적카드${STAMP}` })
    assert.equal(await ledgerRow.getByRole('checkbox', { name: '전월실적 제외' }).isEnabled(), true)
  } finally {
    const left = await gql(`query($f: TransactionFilter!) { transactions(filter: $f) { items { id } } }`, {
      f: { month: '2026-08', paymentMethodId: card.id },
    })
    for (const t of left.data.transactions.items) await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: t.id })
    await gql(`mutation($id: Int!) { deletePaymentMethod(id: $id) { deleted } }`, { id: card.id })
  }
})

await step('가족카드: 카드 상세·카드 추가에서 추가 → 실적·혜택·결제 예정 금액은 본인 카드에 합산, 대시보드 실적엔 본인 카드만', async () => {
  const parent = await createCard(
    { name: `E2E본인${STAMP}`, kind: 'CREDIT_CARD', issuer: '현대', paymentDay: 25, closingDay: 11, closingMonthOffset: 0 },
    { issuer: '현대', monthlyTarget: 300000, annualFee: 5000, benefits: [{ name: '할인10', rate: 0.1, sharesCardLimit: false }] },
  )
  const benefitId = parent.benefits[0].id
  const familyIds = []
  const txIds = []
  try {
    // 카드 상세의 가족카드 추가: 가족카드 체크·본인 카드가 골라진 채로 열리고, 정산 일정·카드 상품 입력은 없다
    await page.goto(`${BASE}/cards/${parent.id}?month=2026-08`)
    await page.getByRole('button', { name: '+ 가족카드 추가' }).click()
    const modal = page.getByRole('dialog')
    assert.equal(await modal.getByRole('checkbox', { name: '가족카드' }).isChecked(), true)
    assert.equal(await modal.getByLabel('본인 카드 *').inputValue(), String(parent.id))
    assert.equal(await modal.getByLabel('이름 *').inputValue(), `E2E본인${STAMP} 가족`)
    assert.equal(await modal.getByLabel('카드사').inputValue(), '현대')
    assert.equal(await modal.getByLabel('결제일').count(), 0)
    assert.equal(await modal.getByRole('combobox', { name: '카드 상품' }).count(), 0)
    assert.equal(await modal.getByRole('button', { name: '새 상품' }).count(), 0)
    await modal.getByLabel('카드번호 끝 4자리').fill('4321')
    await modal.getByRole('button', { name: '추가', exact: true }).click()
    await page.getByRole('heading', { name: `E2E본인${STAMP} 가족`, exact: true }).waitFor()
    const familyId = Number(page.url().match(/cards\/(\d+)/)[1])
    familyIds.push(familyId)
    const info = page.locator('dl.kv')
    await info.getByText('매월 25일 결제 · 전월 12일 ~ 당월 11일 이용').waitFor()
    await info.getByText('5,000원').waitFor() // 연회비는 본인 카드의 상품 것
    await info.getByRole('link', { name: `E2E본인${STAMP}`, exact: true }).waitFor()
    assert.equal(await page.getByRole('button', { name: '+ 가족카드 추가' }).count(), 0, '가족카드 아래에는 가족카드를 두지 않는다')

    // 가족카드 거래에 본인 카드 혜택을 고르고, 실적·혜택은 본인 카드와 합친다
    for (const [pm, amount, benefitIds] of [
      [familyId, 200000, [benefitId]],
      [parent.id, 150000, []],
    ]) {
      const tx = await gql(`mutation($input: TransactionInput!) { createTransaction(input: $input) { id benefitIds } }`, {
        input: { date: '2026-08-10', amount, paymentMethodId: pm, description: `E2E가족결제${STAMP}`, benefitIds },
      })
      txIds.push(tx.data.createTransaction.id)
      assert.deepEqual(tx.data.createTransaction.benefitIds, benefitIds)
    }
    const monthly = await gql(`{ cardsMonthly(month: "2026-08") { paymentMethodId spending performance discountTotal } }`)
    const ids = monthly.data.cardsMonthly.map((r) => r.paymentMethodId)
    assert.ok(!ids.includes(familyId), '전월실적 목록에 가족카드는 없다')
    assert.deepEqual(
      monthly.data.cardsMonthly.find((r) => r.paymentMethodId === parent.id),
      { paymentMethodId: parent.id, spending: 150000, performance: 350000, discountTotal: 20000 },
    )
    await page.goto(`${BASE}/cards/${parent.id}?month=2026-08`)
    // 실적·혜택과 결제 예정 금액 둘 다 가족카드 이용을 합친다
    const included = page.getByText(`가족카드 E2E본인${STAMP} 가족 이용 포함`)
    await included.first().waitFor()
    assert.equal(await included.count(), 2)
    await page.locator('.stat-value', { hasText: '350,000원' }).waitFor()
    await page.locator('dl.kv').getByRole('link', { name: `E2E본인${STAMP} 가족` }).waitFor()

    // 대시보드 카드 실적 현황: 본인 카드는 있고 가족카드는 없다
    await page.goto(`${BASE}/?month=2026-08`)
    const plan = page.locator('section.panel', { has: page.getByRole('heading', { name: '카드 실적 현황' }) })
    await plan.getByRole('link', { name: `E2E본인${STAMP}`, exact: true }).waitFor()
    assert.equal(await plan.getByRole('link', { name: `E2E본인${STAMP} 가족` }).count(), 0)

    // 카드별 내역: 가족카드는 본인 카드 바로 아래, 이용금액은 따로이고 리포트는 본인 카드 것
    await page.goto(`${BASE}/card-transactions?month=2026-08&card=${familyId}`)
    const side = page.locator('.side-list button')
    await side.filter({ hasText: `E2E본인${STAMP} 가족` }).waitFor()
    const names = await side.locator('b').allTextContents()
    assert.equal(names[names.indexOf(`E2E본인${STAMP}`) + 1], `E2E본인${STAMP} 가족`)
    await side.filter({ hasText: `E2E본인${STAMP} 가족` }).getByText('200,000').waitFor()
    await page.getByText(`본인 카드 E2E본인${STAMP}의 실적·혜택을 함께 씁니다`).waitFor()
    await page.locator('table.sheet tbody tr', { hasText: `E2E가족결제${STAMP}` }).getByText('할인10').waitFor()

    // 결제 예정 금액: 가족카드 이용은 본인 카드 청구서에 합쳐진다 (가족카드 청구서는 그 카드 이용분)
    const bill = () => `statements(from: "2026-08", to: "2026-08") { confirmedAmount }`
    const bills = await gql(`{ p: paymentMethod(id: ${parent.id}) { ${bill()} } f: paymentMethod(id: ${familyId}) { ${bill()} } }`)
    assert.equal(bills.data.p.statements[0].confirmedAmount, 350000)
    assert.equal(bills.data.f.statements[0].confirmedAmount, 200000)
    await page.goto(`${BASE}/card-transactions?month=2026-08&card=${parent.id}&mode=statement`)
    await page.getByText(`가족카드 E2E본인${STAMP} 가족 이용 포함`).waitFor()
    const familyLine = page.locator('table tbody tr', { hasText: `E2E본인${STAMP} 가족` })
    await familyLine.getByText('200,000').waitFor()
    await page.getByText('결제 금액 (2건)').waitFor()

    // 카드 추가 화면: 가족카드를 체크하고 본인 카드를 고른다
    await page.goto(`${BASE}/cards`)
    await page.getByRole('button', { name: '+ 카드 추가' }).click()
    const add = page.getByRole('dialog')
    await add.getByLabel('결제일').waitFor()
    await add.getByRole('checkbox', { name: '가족카드' }).check()
    assert.equal(await add.getByLabel('결제일').count(), 0)
    await add.getByLabel('본인 카드 *').selectOption(String(parent.id))
    await add.getByLabel('이름 *').fill(`E2E가족둘${STAMP}`)
    await add.getByRole('button', { name: '추가', exact: true }).click()
    await page.getByRole('heading', { name: `E2E가족둘${STAMP}`, exact: true }).waitFor()
    familyIds.push(Number(page.url().match(/cards\/(\d+)/)[1]))
    const second = await gql(`query($id: Int!) { paymentMethod(id: $id) { parentId kind paymentDay issuer benefits { id } } }`, { id: familyIds[1] })
    assert.deepEqual(second.data.paymentMethod, { parentId: parent.id, kind: 'CREDIT_CARD', paymentDay: 25, issuer: '현대', benefits: [{ id: benefitId }] })
  } finally {
    for (const id of txIds) await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id })
    for (const id of familyIds) await gql(`mutation($id: Int!) { deletePaymentMethod(id: $id) { deleted } }`, { id })
    const del = await gql(`mutation($id: Int!) { deletePaymentMethod(id: $id) { deleted } }`, { id: parent.id })
    assert.equal(del.data.deletePaymentMethod.deleted, true, '가족카드를 먼저 지우면 본인 카드도 지워진다')
  }
})

await step('은행 잔고: 계좌 추가 → 잔고 기록 → 이력', async () => {
  await page.goto(`${BASE}/accounts`)
  await page.getByRole('button', { name: '+ 계좌 추가' }).click()
  const modal = page.getByRole('dialog')
  await modal.getByLabel('이름 *').fill(`E2E통장${STAMP}`)
  await modal.getByLabel('잔고', { exact: true }).fill('1,000,000')
  await modal.getByRole('button', { name: '저장' }).click()
  const row = page.locator('tbody tr', { hasText: `E2E통장${STAMP}` })
  await row.waitFor()
  await row.getByLabel(`E2E통장${STAMP} 잔고`).fill('1234567')
  await row.getByRole('button', { name: '기록' }).click()
  await row.getByText('1,234,567').waitFor()
  await row.getByRole('button', { name: '이력' }).click()
  await page.getByRole('dialog').getByText('+234,567').waitFor()
  await page.getByRole('button', { name: '닫기' }).click()
})

await step('은행 잔고: 기준일 없는 계좌는 편집해도 잔고 기록이 생기지 않는다', async () => {
  const before = await gql(`{ accounts { id name balanceDate history { id } } }`)
  const target = before.data.accounts.find((a) => a.balanceDate == null)
  assert.ok(target, '기준일 없는 계좌가 필요합니다 (청약종합저축)')
  await page.goto(`${BASE}/accounts`)
  await page.locator('tbody tr', { hasText: target.name }).getByRole('button', { name: '편집' }).click()
  const modal = page.getByRole('dialog')
  await modal.getByLabel('메모').fill(`E2E 메모 ${STAMP}`)
  await modal.getByRole('button', { name: '저장' }).click()
  await modal.waitFor({ state: 'detached' })
  const after = await gql(`query($id: Int!) { account(id: $id) { balanceDate memo history { id } } }`, { id: target.id })
  assert.equal(after.data.account.balanceDate, null)
  assert.equal(after.data.account.history.length, target.history.length)
  assert.equal(after.data.account.memo, `E2E 메모 ${STAMP}`)
})

await step('지출 계획: 반복 지출 추가 → 예정 거래 생성 → 삭제', async () => {
  await page.goto(`${BASE}/plan`)
  await page.getByRole('button', { name: '+ 반복 항목 추가' }).click()
  const modal = page.getByRole('dialog')
  await modal.getByLabel('내용 *').fill(`E2E구독${STAMP}`)
  await modal.getByLabel('금액 *').fill('9900')
  await modal.getByLabel('날짜').selectOption('31')
  // 자동입력 금액 범위: '이 금액만'은 시작·끝을 예정 금액으로 채운다
  await modal.getByRole('button', { name: '이 금액만' }).click()
  assert.equal(await modal.getByLabel('금액 범위 시작').inputValue(), '9900')
  assert.equal(await modal.getByLabel('금액 범위 끝').inputValue(), '9900')
  await modal.getByRole('button', { name: '저장' }).click()
  await page.locator('tbody tr', { hasText: `E2E구독${STAMP}` }).getByText('매칭: 9,900원만').waitFor()
  const r = await gql(`{ transactions(filter: {search: "E2E구독${STAMP}", statuses: [PLANNED]}) { totalCount items { date } } }`)
  assert.ok(r.data.transactions.totalCount >= 3, `planned=${r.data.transactions.totalCount}`)
  const rules = await gql(`{ recurringRules { description matchAmountMin matchAmountMax } }`)
  const saved = rules.data.recurringRules.find((x) => x.description === `E2E구독${STAMP}`)
  assert.deepEqual([saved.matchAmountMin, saved.matchAmountMax], [9900, 9900])
  acceptNextDialog()
  await page.locator('tbody tr', { hasText: `E2E구독${STAMP}` }).getByRole('button', { name: '삭제' }).click()
  await page.locator('tbody tr', { hasText: `E2E구독${STAMP}` }).waitFor({ state: 'detached' })
  const left = await gql(`{ transactions(filter: {search: "E2E구독${STAMP}", statuses: [PLANNED]}) { totalCount } }`)
  assert.equal(left.data.transactions.totalCount, 0)
})

await step('지출 계획: 반복 항목에 카드 혜택 → 예정 거래에 붙고, 결제수단을 바꾸면 비워진다', async () => {
  const { data } = await gql(`{ paymentMethods { id name benefits { id name } } }`)
  const card = data.paymentMethods.find((p) => p.benefits.length > 0)
  const other = data.paymentMethods.find((p) => p.id !== card?.id)
  assert.ok(card && other, '혜택 있는 카드와 다른 결제수단이 있어야 합니다')
  const benefit = card.benefits[0]
  await page.goto(`${BASE}/plan`)
  await page.getByRole('button', { name: '+ 반복 항목 추가' }).click()
  const modal = page.getByRole('dialog')
  await modal.getByLabel('내용 *').fill(`E2E혜택${STAMP}`)
  await modal.getByLabel('금액 *').fill('15000')
  // label로 감싼 select의 이름에는 고른 옵션 글자도 붙는다 ("결제수단 선택 안 함")
  const pmSelect = modal.getByRole('combobox', { name: /^결제수단/ })
  await pmSelect.selectOption(String(card.id))
  const check = modal.getByRole('group', { name: `${card.name} 혜택` }).getByRole('checkbox').first()
  await check.check()
  await pmSelect.selectOption(String(other.id))
  await pmSelect.selectOption(String(card.id))
  assert.equal(await check.isChecked(), false, '결제수단을 바꾸면 고른 혜택이 비워져야 합니다')
  await check.check()
  await modal.getByRole('button', { name: '저장' }).click()
  const row = page.locator('tbody tr', { hasText: `E2E혜택${STAMP}` })
  await row.getByText(`혜택: ${benefit.name}`).waitFor()
  const r = await gql(`{ transactions(filter: {search: "E2E혜택${STAMP}", statuses: [PLANNED]}) { totalCount items { benefitIds } } }`)
  assert.ok(r.data.transactions.totalCount >= 1, `planned=${r.data.transactions.totalCount}`)
  for (const t of r.data.transactions.items) assert.deepEqual(t.benefitIds, [benefit.id])
  acceptNextDialog()
  await row.getByRole('button', { name: '삭제' }).click()
  await row.waitFor({ state: 'detached' })
})

await step('지출 계획: 반복 장부 이체 추가 → 받는 장부에 예정 이체 → 삭제', async () => {
  const { data } = await gql(`{ ledgers { id name isDefault } }`)
  const from = data.ledgers.find((l) => l.isDefault) ?? data.ledgers[0]
  const to = data.ledgers.find((l) => l.id !== from.id)
  assert.ok(to, '장부가 둘 이상 있어야 합니다')
  await page.goto(`${BASE}/plan?ledger=${from.id}`)
  await page.getByRole('button', { name: '+ 반복 항목 추가' }).click()
  const modal = page.getByRole('dialog')
  await modal.getByRole('button', { name: '장부 이체' }).click()
  await modal.getByLabel('내용 *').fill(`E2E이체${STAMP}`)
  await modal.getByLabel('금액 *').fill('123000')
  await modal.getByLabel('받는 장부').selectOption(String(to.id))
  await modal.getByRole('button', { name: '저장' }).click()
  const ruleRow = page.locator('tbody tr', { hasText: `E2E이체${STAMP}` })
  await ruleRow.getByText(`→ ${to.name}`).waitFor()
  const r = await gql(`{ transactions(filter: {search: "E2E이체${STAMP}", statuses: [PLANNED], ledgerId: ${to.id}}) { totalCount items { ledgerId transferLedgerId categoryId } } }`)
  assert.ok(r.data.transactions.totalCount >= 3, `planned=${r.data.transactions.totalCount}`)
  assert.deepEqual(r.data.transactions.items[0], { ledgerId: from.id, transferLedgerId: to.id, categoryId: null })
  // 받는 장부의 가계부에서는 '← 보낸 장부'로 보이고 항목은 바꿀 수 없다
  await page.goto(`${BASE}/transactions?ledger=${to.id}&status=PLANNED`)
  await page.locator('table.sheet tbody tr', { hasText: `E2E이체${STAMP}` }).first().getByText(`← ${from.name}`).waitFor()
  await page.goto(`${BASE}/plan?ledger=${from.id}`)
  acceptNextDialog()
  await page.locator('tbody tr', { hasText: `E2E이체${STAMP}` }).getByRole('button', { name: '삭제' }).click()
  await page.locator('tbody tr', { hasText: `E2E이체${STAMP}` }).waitFor({ state: 'detached' })
  const left = await gql(`{ transactions(filter: {search: "E2E이체${STAMP}", statuses: [PLANNED]}) { totalCount } }`)
  assert.equal(left.data.transactions.totalCount, 0)
})

await step('설정: API 토큰 발급 → 토큰으로 자동 입력 → 폐기 후 거부', async () => {
  await page.goto(`${BASE}/settings`)
  await page.getByPlaceholder('토큰 이름 (예: 아이폰 단축어)').fill(`e2e-${STAMP}`)
  await page.getByRole('button', { name: '발급' }).click()
  const token = (await page.getByRole('dialog').locator('pre').innerText()).trim()
  assert.match(token, /^cb_[A-Za-z0-9]{40}$/)
  await page.getByRole('button', { name: '닫기' }).click()

  const sms = `[Web발신]\n신한카드(1234)승인 홍*동 7,700원(일시불)10/22 12:34 E2E분식${STAMP} 누적1,234,567원`
  const post = (headers) => fetch(`${BASE}/api/ingest`, { method: 'POST', headers: { 'Content-Type': 'text/plain', ...headers }, body: sms })
  assert.equal((await post({})).status, 401)
  const ok = await post({ Authorization: `Bearer ${token}` })
  assert.equal(ok.status, 200)
  const body = await ok.json()
  assert.equal(body.created, true)
  assert.equal(body.transaction.amount, 7700)
  assert.equal(body.transaction.paymentMethod, '알뜰교통신한')
  const dup = await (await post({ Authorization: `Bearer ${token}` })).json()
  assert.equal(dup.duplicate, true)

  // 다른 사이트에서 보낸 요청(Origin 불일치)은 토큰이 있어도 거부
  const evil = await post({ Authorization: `Bearer ${token}`, Origin: 'https://evil.example' })
  assert.equal(evil.status, 403)
  // 같은 토큰으로 GraphQL도 쓴다 (토큰 권한은 하나)
  const gqlWith = (t) =>
    fetch(`${BASE}/api/graphql`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${t}` },
      body: JSON.stringify({ query: '{ appInfo { version } }' }),
    })
  assert.equal((await gqlWith(token)).status, 200)

  await page.reload()
  const row = page.locator('tbody tr', { hasText: `e2e-${STAMP}` })
  await row.waitFor()
  acceptNextDialog()
  await row.getByRole('button', { name: '폐기' }).click()
  await row.waitFor({ state: 'detached' })
  assert.equal((await post({ Authorization: `Bearer ${token}` })).status, 401)
  assert.equal((await gqlWith(token)).status, 401)
  await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: body.transaction.id })
})

await step('로그아웃 → 로그인 화면', async () => {
  await page.goto(`${BASE}/`)
  await page.getByRole('button', { name: '로그아웃' }).click()
  await page.getByRole('link', { name: 'Google로 로그인' }).waitFor()
  assert.equal((await gql('{ appInfo { version } }')).ok, false, '로그아웃 뒤 API는 401')
})

await browser.close()
google.close()
if (pageErrors.length) {
  failures++
  console.log(`✗ 브라우저 콘솔 오류 ${pageErrors.length}건:\n  ${[...new Set(pageErrors)].slice(0, 10).join('\n  ')}`)
}
console.log(failures ? `\n실패 ${failures}건` : '\n모두 통과')
process.exit(failures ? 1 : 0)
