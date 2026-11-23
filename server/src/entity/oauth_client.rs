use sea_orm::entity::prelude::*;

/// MCP 클라이언트(OpenAI Dots·ChatGPT 플러그인, Claude 등)가 동적 등록(RFC 7591)으로 만든 OAuth 클라이언트.
/// 누구나 등록할 수 있고 계정과 무관하다. 계정의 데이터를 읽으려면 그 계정이 로그인해 승인해야 한다(`oauth_grants`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_clients")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 공개 클라이언트 id (`cbc_` + 영숫자)
    #[sea_orm(unique)]
    pub client_id: String,
    /// `client_secret_basic`·`client_secret_post` 클라이언트의 비밀 SHA-256 (공개 클라이언트는 None)
    pub secret_hash: Option<String>,
    pub name: String,
    /// 등록한 redirect_uri 목록 (JSON 문자열 배열)
    pub redirect_uris: String,
    pub created_at: DateTimeUtc,
}

impl Model {
    pub fn redirect_uris(&self) -> Vec<String> {
        serde_json::from_str(&self.redirect_uris).unwrap_or_default()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
