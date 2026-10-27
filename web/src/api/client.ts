// GraphQL/REST 호출. 쿠키 세션을 쓰므로 같은 출처(same-origin)로만 호출한다.

export class AuthRequiredError extends Error {
  constructor() {
    super('로그인이 필요합니다')
  }
}

export class GraphQLRequestError extends Error {}

export const UNAUTHORIZED_EVENT = 'cashbook:unauthorized'

export async function gql<T>(query: string, variables?: Record<string, unknown>): Promise<T> {
  const res = await fetch('/api/graphql', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify({ query, variables }),
  })
  if (res.status === 401) {
    window.dispatchEvent(new Event(UNAUTHORIZED_EVENT))
    throw new AuthRequiredError()
  }
  let json: { data?: T; errors?: { message: string }[] }
  try {
    json = await res.json()
  } catch {
    throw new GraphQLRequestError(`서버 응답을 읽을 수 없습니다 (HTTP ${res.status})`)
  }
  if (json.errors?.length) {
    throw new GraphQLRequestError(json.errors.map((e) => e.message).join('\n'))
  }
  if (!json.data) {
    throw new GraphQLRequestError(`빈 응답 (HTTP ${res.status})`)
  }
  return json.data
}

export interface SessionInfo {
  authRequired: boolean
  authenticated: boolean
}

export async function fetchSession(): Promise<SessionInfo> {
  const res = await fetch('/api/session', { credentials: 'same-origin' })
  if (!res.ok) throw new Error(`세션 확인 실패 (HTTP ${res.status})`)
  return res.json()
}

export async function login(password: string): Promise<SessionInfo> {
  const res = await fetch('/api/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'same-origin',
    body: JSON.stringify({ password }),
  })
  const json = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(json.error ?? `로그인 실패 (HTTP ${res.status})`)
  return json
}

export async function logout(): Promise<void> {
  await fetch('/api/logout', { method: 'POST', credentials: 'same-origin' })
}

export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message
  return String(e)
}
