use sea_orm::entity::prelude::*;

/// 계정이 MCP 클라이언트에 허용한 연결 하나. 액세스·리프레시 토큰은 SHA-256 해시만 저장하고,
/// 리프레시할 때마다 둘 다 새로 바꾼다. 지우면 연결이 끊긴다(설정 화면의 연결된 앱).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, async_graphql::SimpleObject)]
#[sea_orm(table_name = "oauth_grants")]
#[graphql(name = "ConnectedApp")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// 계정 (`users.id`)
    #[graphql(skip)]
    pub user_id: i32,
    /// `oauth_clients.id`
    #[graphql(skip)]
    pub oauth_client_id: i32,
    /// 승인할 때 보인 클라이언트 이름
    pub client_name: String,
    /// 허용한 범위 (공백 구분, 지금은 `cashbook:read`만)
    pub scope: String,
    /// 토큰을 쓸 수 있는 리소스 (MCP 주소)
    #[graphql(skip)]
    pub resource: String,
    #[sea_orm(unique)]
    #[graphql(skip)]
    pub access_hash: String,
    #[graphql(skip)]
    pub access_expires_at: DateTimeUtc,
    #[sea_orm(unique)]
    #[graphql(skip)]
    pub refresh_hash: String,
    /// 이때까지 쓰지 않으면 다시 연결해야 한다 (쓸 때마다 늘어난다)
    pub refresh_expires_at: DateTimeUtc,
    pub created_at: DateTimeUtc,
    pub last_used_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

super::user_owned!();
