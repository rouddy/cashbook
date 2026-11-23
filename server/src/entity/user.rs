use sea_orm::entity::prelude::*;

/// 로그인 계정. 장부·카테고리·결제수단·계좌·거래·반복 규칙·API 토큰은 계정마다 따로 갖는다(`user_id`).
/// 카드 상품(전월실적 구간·혜택·연회비)은 계정과 관계없이 함께 쓴다.
///
/// id 1은 계정 기능 이전의 데이터를 가진 계정이다. `google_sub`가 비어 있으면 아직 아무도 넘겨받지 않은 것이고,
/// `CASHBOOK_OWNER_EMAIL` 계정이 처음 로그인할 때 넘겨받는다. 인증 없이 로컬로 띄우면 이 계정으로 동작한다.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Google 계정 고유 id (OpenID `sub`)
    #[sea_orm(unique)]
    pub google_sub: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    /// 프로필 사진 주소
    pub picture: Option<String>,
    pub created_at: DateTimeUtc,
    pub last_login_at: Option<DateTimeUtc>,
}

/// 계정 기능 이전 데이터를 가진 계정 (인증 없이 로컬로 띄울 때의 계정)
pub const LEGACY_USER_ID: i32 = 1;

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
