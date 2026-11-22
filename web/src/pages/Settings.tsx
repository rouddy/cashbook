import { useState, type FormEvent } from 'react'

import { useGql, useMutate, useRefData, type RefMaps } from '../api/hooks'
import { CREATE_LEDGER, CREATE_TOKEN, DELETE_LEDGER, DELETE_PM, DELETE_TOKEN, RESTORE_PM, TOKENS_QUERY, UPDATE_LEDGER } from '../api/queries'
import type { ApiToken, Ledger, PaymentMethod } from '../api/types'
import CardForm from '../components/CardForm'
import { ErrorBox, Loading, Panel } from '../components/Common'
import Modal from '../components/Modal'
import { useToast } from '../components/Toast'
import { dateTime, PAYMENT_KIND_LABEL } from '../lib/format'

export default function Settings() {
  const { ref, error } = useRefData()
  if (error) return <ErrorBox error={error} />
  if (!ref) return <Loading />
  return (
    <div className="stack" style={{ maxWidth: 1100 }}>
      <div className="page-head">
        <h1>설정</h1>
      </div>
      <div className="grid-2" style={{ alignItems: 'start' }}>
        <Ledgers refMaps={ref} />
        <OtherPaymentMethods refMaps={ref} />
      </div>
      <ApiTokens />
      <ApiGuide authEnabled={ref.appInfo.authEnabled} />
    </div>
  )
}

function Ledgers({ refMaps }: { refMaps: RefMaps }) {
  const { run, pending } = useMutate()
  const toast = useToast()
  const [name, setName] = useState('')
  const add = async (e: FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    if (await run(CREATE_LEDGER, { input: { name: name.trim() } })) setName('')
  }
  const rename = (l: Ledger) => {
    const v = window.prompt('장부 이름', l.name)
    if (v && v.trim() && v.trim() !== l.name) run(UPDATE_LEDGER, { id: l.id, input: { name: v.trim(), description: l.description } })
  }
  return (
    <Panel title="장부" sub="장부마다 카테고리와 지출 계획을 따로 관리합니다. 기본 장부는 자동 입력과 대시보드의 기본값입니다." flush>
      <table className="table">
        <tbody>
          {refMaps.ledgers.map((l) => (
            <tr key={l.id}>
              <td>
                <b>{l.name}</b> {l.isDefault && <span className="badge planned">기본</span>}
                {l.description && <div className="small muted">{l.description}</div>}
              </td>
              <td className="nowrap" style={{ textAlign: 'right' }}>
                {!l.isDefault && (
                  <button className="btn sm ghost" disabled={pending} onClick={() => run(UPDATE_LEDGER, { id: l.id, input: { name: l.name, description: l.description, isDefault: true } })}>
                    기본으로
                  </button>
                )}
                <button className="btn sm ghost" onClick={() => rename(l)}>
                  이름 변경
                </button>
                <button
                  className="btn sm ghost danger"
                  disabled={pending}
                  onClick={async () => {
                    if (!window.confirm(`'${l.name}' 장부를 삭제할까요? (거래가 없을 때만 가능)`)) return
                    if (await run(DELETE_LEDGER, { id: l.id })) toast('삭제했습니다')
                  }}
                >
                  삭제
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <form className="row" style={{ padding: 12 }} onSubmit={add}>
        <input className="input" placeholder="새 장부 이름" value={name} onChange={(e) => setName(e.target.value)} />
        <button className="btn" disabled={!name.trim() || pending}>
          추가
        </button>
      </form>
    </Panel>
  )
}

function OtherPaymentMethods({ refMaps }: { refMaps: RefMaps }) {
  const { run, pending } = useMutate()
  const [editing, setEditing] = useState<PaymentMethod | 'new' | null>(null)
  const list = refMaps.paymentMethods.filter((p) => !p.isCard)
  return (
    <Panel
      title="카드 외 결제수단"
      sub="현금, 계좌이체 등. 연결 계좌를 지정하면 잔고 기준일 이후 출금을 여유자금에서 뺍니다."
      actions={
        <button className="btn sm" onClick={() => setEditing('new')}>
          + 추가
        </button>
      }
      flush
    >
      <table className="table">
        <tbody>
          {list.map((p) => (
            <tr key={p.id} style={p.archived ? { opacity: 0.55 } : undefined}>
              <td>
                <b>{p.name}</b> <span className="badge">{PAYMENT_KIND_LABEL[p.kind]}</span> {p.archived && <span className="badge">보관</span>}
                {p.accountId && <div className="small muted">연결 계좌: {refMaps.accountById.get(p.accountId)?.name}</div>}
                {p.memo && <div className="small muted">{p.memo}</div>}
              </td>
              <td className="nowrap" style={{ textAlign: 'right' }}>
                <button className="btn sm ghost" onClick={() => setEditing(p)}>
                  편집
                </button>
                {p.archived ? (
                  <button className="btn sm ghost" disabled={pending} onClick={() => run(RESTORE_PM, { id: p.id })}>
                    다시 사용
                  </button>
                ) : (
                  <button className="btn sm ghost danger" disabled={pending} onClick={() => window.confirm(`'${p.name}'을(를) 삭제(거래가 있으면 보관)할까요?`) && run(DELETE_PM, { id: p.id })}>
                    삭제
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {editing && (
        <Modal title={editing === 'new' ? '결제수단 추가' : '결제수단 편집'} onClose={() => setEditing(null)}>
          <CardForm
            card={editing === 'new' ? undefined : editing}
            refMaps={refMaps}
            kinds={['CASH', 'BANK_TRANSFER', 'OTHER']}
            onDone={() => setEditing(null)}
            onCancel={() => setEditing(null)}
          />
        </Modal>
      )}
    </Panel>
  )
}

function ApiTokens() {
  const q = useGql<{ apiTokens: ApiToken[] }>(['tokens'], TOKENS_QUERY)
  const { run, pending } = useMutate()
  const [name, setName] = useState('')
  const [created, setCreated] = useState<string | null>(null)
  const toast = useToast()

  const create = async (e: FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    const r = await run<{ createApiToken: { token: string } }>(CREATE_TOKEN, { name: name.trim() })
    if (!r) return
    setCreated(r.createApiToken.token)
    setName('')
  }

  return (
    <Panel
      title="API 토큰"
      sub="휴대폰 자동화 앱·가계부 앱·스크립트가 쓰는 키. 자동 입력과 GraphQL(내역 읽기·쓰기)을 모두 쓸 수 있으니 기기·용도마다 따로 발급하고, 잃어버리거나 유출되면 바로 폐기하세요. 원문은 발급할 때 한 번만 보여줍니다."
      flush
    >
      {q.error ? (
        <ErrorBox error={q.error} />
      ) : (
        <table className="table">
          <thead>
            <tr>
              <th>이름</th>
              <th>앞자리</th>
              <th>발급</th>
              <th>마지막 사용</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {(q.data?.apiTokens ?? []).map((t) => (
              <tr key={t.id}>
                <td>{t.name}</td>
                <td>
                  <code>{t.tokenPrefix}…</code>
                </td>
                <td className="small">{dateTime(t.createdAt)}</td>
                <td className="small">{t.lastUsedAt ? dateTime(t.lastUsedAt) : <span className="muted">없음</span>}</td>
                <td>
                  <button className="btn sm ghost danger" disabled={pending} onClick={() => window.confirm(`'${t.name}' 토큰을 폐기할까요? 이 토큰을 쓰는 자동화는 더 이상 동작하지 않습니다.`) && run(DELETE_TOKEN, { id: t.id })}>
                    폐기
                  </button>
                </td>
              </tr>
            ))}
            {q.data?.apiTokens.length === 0 && (
              <tr>
                <td colSpan={5} className="empty">
                  발급한 토큰이 없습니다
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
      <form className="row" style={{ padding: 12 }} onSubmit={create}>
        <input className="input" placeholder="토큰 이름 (예: 아이폰 단축어)" value={name} onChange={(e) => setName(e.target.value)} style={{ width: 240 }} />
        <button className="btn primary" disabled={!name.trim() || pending}>
          발급
        </button>
      </form>
      {created && (
        <Modal title="토큰이 발급되었습니다" onClose={() => setCreated(null)}>
          <p className="text-2">지금 복사해 두세요. 이 창을 닫으면 다시 볼 수 없습니다.</p>
          <pre>{created}</pre>
          <button
            className="btn primary"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(created)
                toast('복사했습니다')
              } catch {
                toast('복사하지 못했습니다. 직접 선택해 복사하세요', 'error')
              }
            }}
          >
            복사
          </button>
        </Modal>
      )}
    </Panel>
  )
}

function ApiGuide({ authEnabled }: { authEnabled: boolean }) {
  const base = window.location.origin
  const auth = authEnabled ? ` \\\n  -H 'Authorization: Bearer cb_발급한토큰'` : ''
  return (
    <Panel title="자동 입력 API 사용법" sub="여러 플랫폼에서 결제 내역을 자동으로 넣는 방법">
      {!authEnabled && (
        <div className="notice" style={{ marginBottom: 12 }}>
          <span className="notice-icon">!</span>
          <span>
            지금은 비밀번호(<code>CASHBOOK_PASSWORD</code>) 없이 로컬에서만 실행 중이라 토큰 없이 호출됩니다. 휴대폰 등 외부에서 보내려면 서버에 비밀번호를 설정하고 외부 주소로 열어야 하며, 그때부터는 토큰이 필요합니다.
          </span>
        </div>
      )}
      <div className="stack" style={{ gap: 12 }}>
        <div>
          <h3>1. 카드 승인 문자 원문 그대로 보내기</h3>
          <p className="small text-2">금액·가맹점·일시·할부·취소·카드사·끝자리를 서버가 해석하고, 과거 내역으로 항목을 자동 분류합니다. 같은 문자를 다시 보내도 한 번만 저장됩니다.</p>
          <pre>{`curl -X POST ${base}/api/ingest${auth} \\
  -H 'Content-Type: text/plain' \\
  --data-binary '[Web발신] 신한카드(1234)승인 홍*동 12,500원(일시불)10/23 12:34 스타벅스 누적1,234,567원'`}</pre>
        </div>
        <div>
          <h3>2. JSON으로 보내기</h3>
          <pre>{`curl -X POST ${base}/api/ingest${auth} \\
  -H 'Content-Type: application/json' \\
  -d '{"amount": 12500, "merchant": "스타벅스", "card": "MG+S", "date": "2026-10-23",
       "category": "외식", "ledger": "가계부", "externalId": "pay-20261023-001"}'`}</pre>
          <table className="table small">
            <tbody>
              {[
                ['amount', '금액(원). 없으면 text에서 찾음'],
                ['merchant / description', '내용(가맹점)'],
                ['card / paymentMethod', '결제수단 이름·별칭 또는 카드번호 끝 4자리'],
                ['date', 'YYYY-MM-DD (없으면 문자 날짜 → 오늘)'],
                ['category, ledger, member, memo', '이름으로 지정 (없으면 과거 내역으로 추천)'],
                ['installmentMonths', '할부 개월 수'],
                ['cancel', 'true면 취소/환불 (음수로 저장)'],
                ['text', '승인 문자 원문 (명시 필드가 우선)'],
                ['externalId', '중복 방지 키. 같은 키는 한 번만 저장'],
                ['planned', 'true면 예정 거래로 저장'],
              ].map(([k, v]) => (
                <tr key={k}>
                  <td>
                    <code>{k}</code>
                  </td>
                  <td className="text-2">{v}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="small text-2">배열로 여러 건을 한 번에 보낼 수도 있습니다. 같은 결제수단의 ±7일 예정 항목과 금액이나 내용(또는 반복 규칙의 매칭 키워드)이 맞으면 새로 만들지 않고 그 예정 항목을 확정합니다. 반복 규칙에 금액 범위나 매칭 키워드를 정했으면 그 조건도 맞아야 합니다.</p>
        </div>
        <div>
          <h3>3. 아이폰 (단축어 앱)</h3>
          <ol className="small text-2" style={{ margin: 0, paddingLeft: 18 }}>
            <li>단축어 → 자동화 → <b>메시지</b> → 보낸 사람/내용에 "승인" 포함 → 즉시 실행</li>
            <li>동작: <b>URL의 콘텐츠 가져오기</b> — URL <code>{base}/api/ingest</code>, 방법 POST</li>
            <li>헤더: <code>Content-Type: text/plain</code>{authEnabled && <>, <code>Authorization: Bearer 토큰</code></>}</li>
            <li>요청 본문: 파일 → 단축어 입력의 <b>메시지 내용</b></li>
          </ol>
        </div>
        <div>
          <h3>4. 안드로이드 (가계부 자동 입력 앱 / 문자 전달 앱 / MacroDroid / Tasker)</h3>
          <p className="small text-2" style={{ margin: 0 }}>
            <b>가계부 자동 입력 앱</b>(저장소의 <code>android/</code>): 서버 주소에 <code>{base}</code>
            {authEnabled ? ', API 토큰에 발급한 토큰' : ''}을 넣고, 카드사 문자 번호나 결제 앱 알림을 규칙으로 등록하면 받는 즉시 올립니다
            (문자 형식 템플릿, 장문 LMS, 재시도·기록).
          </p>
          <p className="small text-2" style={{ margin: 0 }}>
            다른 앱을 쓸 때: 카드사 번호에서 온 문자를 받으면 HTTP POST로 <code>{base}/api/ingest</code>에 문자 내용(본문)을 보내도록 설정합니다. Content-Type은 <code>text/plain</code>
            {authEnabled ? ', 헤더에 Authorization: Bearer 토큰' : ''}. JSON 템플릿을 쓸 수 있으면 <code>{'{"text": "%문자내용%"}'}</code> 형태도 됩니다.
          </p>
        </div>
        <div>
          <h3>5. GraphQL</h3>
          <pre>{`mutation { ingestTransaction(input: {text: "..."}) { created duplicate matchedPlanned warnings transaction { id amount description } } }`}</pre>
          <p className="small text-2" style={{ margin: 0 }}>
            모든 기능은 <code>{base}/api/graphql</code>에서 쓸 수 있습니다 (로그인 세션 또는 API 토큰). <a href="/api/graphiql">GraphiQL 탐색기</a>
          </p>
        </div>
      </div>
    </Panel>
  )
}
