// E2E 스모크 테스트: 실제 브라우저(시스템 Chrome)로 주요 화면과 입력 흐름을 확인한다.
//
// 테스트용 DB로 띄운 서버가 필요하다 (실제 데이터를 쓰지 말 것). 예:
//   sqlite3 ../server/data/cashbook.db ".backup /tmp/cashbook-e2e.db"
//   (cd ../server && DATABASE_URL="sqlite:///tmp/cashbook-e2e.db?mode=rwc" CASHBOOK_PASSWORD=e2e-pass \
//      CASHBOOK_SECRET_FILE=/tmp/cashbook-e2e.key ROCKET_PORT=8001 cargo run)
//   npm run e2e
//
// 환경 변수: E2E_BASE_URL(기본 http://127.0.0.1:8001), E2E_PASSWORD(기본 e2e-pass), CHROME_PATH

import assert from 'node:assert/strict'
import { chromium } from 'playwright-core'

const BASE = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:8001'
const PASSWORD = process.env.E2E_PASSWORD ?? 'e2e-pass'
const CHROME = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const STAMP = Date.now().toString(36)

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

function acceptNextDialog() {
  page.once('dialog', (d) => d.accept())
}

await step('로그인 (틀린 비밀번호 → 오류, 맞는 비밀번호 → 대시보드)', async () => {
  await page.goto(`${BASE}/`)
  await page.getByLabel('비밀번호').fill('wrong-password')
  await page.getByRole('button', { name: '로그인' }).click()
  await page.getByText('비밀번호가 맞지 않습니다').waitFor()
  await page.getByLabel('비밀번호').fill(PASSWORD)
  await page.getByRole('button', { name: '로그인' }).click()
  await page.getByRole('heading', { name: '대시보드' }).waitFor()
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
  await created.getByRole('checkbox').check()
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
  await modal.getByLabel('전월실적 목표').fill('300000')
  await modal.getByRole('button', { name: '+ 혜택 추가' }).click()
  const benefitRow = modal.locator('tbody tr').last()
  await benefitRow.locator('input').nth(0).fill('커피')
  await benefitRow.locator('input').nth(1).fill('10')
  await modal.getByRole('button', { name: '+ 구간 추가' }).click()
  await modal.getByLabel('전월실적 이상').fill('300000')
  await modal.getByLabel('혜택 한도').fill('10000')
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
  const created = await gql(
    `mutation($input: PaymentMethodInput!) { createPaymentMethod(input: $input) { id benefits { id name } } }`,
    {
      input: {
        name: `E2E혜택${STAMP}`,
        kind: 'CREDIT_CARD',
        benefits: [
          { name: '백화점', rate: 0.01 },
          { name: '할인점', rate: 0.01 },
          { name: '간편결제', rate: 0.03, stackable: true },
          { name: '기본적립', kind: 'POINT', rate: 0.001, sharesCardLimit: false },
        ],
        tiers: [{ minSpending: 0, benefitLimit: 3000 }],
      },
    },
  )
  const card = created.data.createPaymentMethod
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
  // 종류를 마일리지로 바꾸면 혜택률을 "N원당 1마일"로 다시 적는다 (0.1% → 1,000원당 1마일)
  await page.goto(`${BASE}/cards/${card.id}?edit=1&month=2026-08`)
  const form = page.locator('form').first()
  const pointRow = form.locator('tbody tr').nth(3)
  await pointRow.getByLabel('혜택 종류').selectOption('MILEAGE')
  assert.equal(await pointRow.getByLabel('1마일당 이용금액').inputValue(), '1000')
  await form.getByRole('button', { name: '저장' }).click()
  await page.locator('tr', { hasText: '기본적립' }).getByText('1,000원당 1마일').waitFor()
  const saved = await gql(`query($id: Int!) { paymentMethod(id: $id) { benefits { name kind rate stackable } } }`, { id: card.id })
  assert.deepEqual(saved.data.paymentMethod.benefits[3], { name: '기본적립', kind: 'MILEAGE', rate: 0.001, stackable: false })
  await gql(`mutation($id: Int!) { deleteTransaction(id: $id) }`, { id: txId })
  const del = await gql(`mutation($id: Int!) { deletePaymentMethod(id: $id) { deleted } }`, { id: card.id })
  assert.equal(del.data.deletePaymentMethod.deleted, true)
})

await step('가계부: 확정 거래의 상태 칸 혜택 버튼으로 카드 혜택 고르기', async () => {
  const created = await gql(
    `mutation($input: PaymentMethodInput!) { createPaymentMethod(input: $input) { id benefits { id name } } }`,
    {
      input: {
        name: `E2E상태칸카드${STAMP}`,
        kind: 'CREDIT_CARD',
        benefits: [
          { name: '백화점', rate: 0.01 },
          { name: '간편결제', rate: 0.03, stackable: true },
        ],
      },
    },
  )
  const card = created.data.createPaymentMethod
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
    const saved = await gql(`query($id: Int!) { transaction(id: $id) { benefitIds } }`, { id: ids[0] })
    assert.deepEqual([...saved.data.transaction.benefitIds].sort(), [id['백화점'], id['간편결제']].sort())
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
  await page.getByLabel('비밀번호').waitFor()
})

await browser.close()
if (pageErrors.length) {
  failures++
  console.log(`✗ 브라우저 콘솔 오류 ${pageErrors.length}건:\n  ${[...new Set(pageErrors)].slice(0, 10).join('\n  ')}`)
}
console.log(failures ? `\n실패 ${failures}건` : '\n모두 통과')
process.exit(failures ? 1 : 0)
