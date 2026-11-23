//! Google 로그인: OAuth 2.0 Authorization Code + PKCE.
//!
//! `/api/auth/google`이 state·PKCE 검증값을 암호화 쿠키에 넣고 Google로 보낸다. Google이 `/api/auth/google/callback`으로
//! 돌려보내면 state를 확인하고, 코드를 토큰으로 바꿔 userinfo(sub·이메일·이름·사진)를 받아 계정에 로그인한다.
//! 토큰은 서버가 Google과 직접(TLS) 주고받으므로 ID 토큰 서명 대신 userinfo 응답을 쓴다.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::GoogleConfig;
use crate::domain::users::GoogleProfile;

/// 로그인 중 상태를 담는 쿠키 (Google에서 돌아오는 요청에도 실리도록 SameSite=Lax)
pub const STATE_COOKIE: &str = "cashbook_oauth";
pub const STATE_MINUTES: i64 = 10;
/// 로그인 뒤 돌아갈 경로의 최대 길이. MCP 승인 화면(`/oauth/authorize?...`)은 OAuth 요청 값을 모두 담아 길다.
/// 로그인 중 쿠키(4KB 한도)에 암호화해 넣으므로 이 정도로 묶는다.
const MAX_NEXT_LEN: usize = 2048;

/// Google로 보낸 로그인 요청 (쿠키에 암호화해 둔다)
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct PendingLogin {
    pub state: String,
    /// PKCE code_verifier
    pub verifier: String,
    /// 로그인 뒤 돌아갈 화면 경로
    pub next: String,
    pub at: i64,
}

impl PendingLogin {
    pub fn new(next: Option<&str>) -> Self {
        Self { state: random(32), verifier: random(64), next: safe_next(next), at: Utc::now().timestamp() }
    }

    pub fn expired(&self) -> bool {
        Utc::now().timestamp() - self.at > STATE_MINUTES * 60
    }
}

fn random(len: usize) -> String {
    rand::rng().sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

/// PKCE S256: base64url(sha256(verifier))
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// 로그인 뒤 돌아갈 경로. 이 사이트 안의 경로만 (다른 사이트로 보내는 열린 리디렉션 방지).
/// 한글 검색어처럼 URI에 그대로 못 쓰는 글자는 퍼센트 인코딩한다 (Location 헤더로 보낼 수 있게).
pub fn safe_next(next: Option<&str>) -> String {
    match next.map(str::trim) {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') && !n.starts_with("/api/") && n.len() <= MAX_NEXT_LEN => {
            let mut out = String::with_capacity(n.len());
            for c in n.chars() {
                if c.is_ascii_graphic() && !"\"<>^`{|}".contains(c) {
                    out.push(c);
                } else {
                    let mut buf = [0u8; 4];
                    for b in c.encode_utf8(&mut buf).bytes() {
                        out.push_str(&format!("%{b:02X}"));
                    }
                }
            }
            out
        }
        _ => "/".into(),
    }
}

/// Google 로그인 화면 주소
pub fn authorize_url(g: &GoogleConfig, pending: &PendingLogin) -> String {
    let mut url = reqwest::Url::parse(&g.auth_url).expect("auth url");
    url.query_pairs_mut()
        .append_pair("client_id", &g.client_id)
        .append_pair("redirect_uri", &g.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &pending.state)
        .append_pair("code_challenge", &challenge(&pending.verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account");
    url.into()
}

/// `/login?error=...` (로그인 화면이 이유를 보여 준다)
pub fn login_error_path(message: &str) -> String {
    let mut url = reqwest::Url::parse("http://localhost/login").expect("static url");
    url.query_pairs_mut().append_pair("error", message);
    format!("/login?{}", url.query().unwrap_or_default())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

/// 인가 코드를 토큰으로 바꾸고 프로필을 받는다. 실패하면 사용자에게 보여줄 이유.
pub async fn fetch_profile(client: &reqwest::Client, g: &GoogleConfig, code: &str, verifier: &str) -> Result<GoogleProfile, String> {
    let res = client
        .post(&g.token_url)
        .form(&[
            ("code", code),
            ("client_id", g.client_id.as_str()),
            ("client_secret", g.client_secret.as_str()),
            ("redirect_uri", g.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|e| format!("Google에 연결하지 못했습니다: {e}"))?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        eprintln!("Google 토큰 교환 실패 ({status}): {body}");
        return Err("Google 로그인을 확인하지 못했습니다. 다시 시도하세요".into());
    }
    let token: TokenResponse = res.json().await.map_err(|e| format!("Google 응답을 읽지 못했습니다: {e}"))?;
    let res = client
        .get(&g.userinfo_url)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|e| format!("Google에 연결하지 못했습니다: {e}"))?;
    if !res.status().is_success() {
        eprintln!("Google 사용자 정보 실패 ({})", res.status());
        return Err("Google 계정 정보를 받지 못했습니다".into());
    }
    res.json().await.map_err(|e| format!("Google 계정 정보를 읽지 못했습니다: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn next_stays_on_this_site() {
        assert_eq!(safe_next(Some("/cards/3?month=2026-11")), "/cards/3?month=2026-11");
        for bad in ["https://evil.example", "//evil.example", "/\\evil", "evil", "/api/graphql", ""] {
            assert_eq!(safe_next(Some(bad)), "/", "{bad}");
        }
        assert_eq!(safe_next(None), "/");
        // Location 헤더로 보낼 수 있게 인코딩한다
        let encoded = safe_next(Some("/transactions?search=점심 값&ledger=1"));
        assert_eq!(encoded, "/transactions?search=%EC%A0%90%EC%8B%AC%20%EA%B0%92&ledger=1");
        assert!(rocket::http::uri::Reference::parse(&encoded).is_ok());
        assert_eq!(safe_next(Some("/cards?month=2026-11&q=%EC%A0%90")), "/cards?month=2026-11&q=%EC%A0%90");
    }

    #[test]
    fn authorize_url_has_pkce_and_state() {
        let g = GoogleConfig {
            client_id: "cid".into(),
            client_secret: "secret".into(),
            redirect_uri: "https://cash.example.com/api/auth/google/callback".into(),
            auth_url: GoogleConfig::AUTH_URL.into(),
            token_url: GoogleConfig::TOKEN_URL.into(),
            userinfo_url: GoogleConfig::USERINFO_URL.into(),
        };
        let p = PendingLogin::new(Some("/cards"));
        let url = reqwest::Url::parse(&authorize_url(&g, &p)).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "cid");
        assert_eq!(q["redirect_uri"], g.redirect_uri);
        assert_eq!(q["state"], p.state);
        assert_eq!(q["code_challenge"], challenge(&p.verifier));
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "openid email profile");
        assert!(!url.as_str().contains("secret"));
        assert_eq!(login_error_path("다시 시도 & 확인"), "/login?error=%EB%8B%A4%EC%8B%9C+%EC%8B%9C%EB%8F%84+%26+%ED%99%95%EC%9D%B8");
    }
}
