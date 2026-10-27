//! 자동 입력 API 토큰 발급/폐기.

use async_graphql::{Context, Object, Result, SimpleObject};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, EntityTrait, QueryOrder, Set};

use super::{db, user_error};
use crate::auth::{generate_token, sha256_hex};
use crate::entity::{api_token, enums::TokenScope};

#[derive(SimpleObject)]
pub struct CreatedApiToken {
    /// 토큰 원문. 지금 한 번만 볼 수 있다.
    pub token: String,
    pub api_token: api_token::Model,
}

#[derive(Default)]
pub struct TokenQuery;

#[Object]
impl TokenQuery {
    async fn api_tokens(&self, ctx: &Context<'_>) -> Result<Vec<api_token::Model>> {
        Ok(api_token::Entity::find().order_by_desc(api_token::Column::Id).all(db(ctx)).await?)
    }
}

#[derive(Default)]
pub struct TokenMutation;

#[Object]
impl TokenMutation {
    /// 토큰 발급 (예: "아이폰 단축어", "갤럭시 문자 전달"). 기본은 자동 입력 전용.
    async fn create_api_token(
        &self,
        ctx: &Context<'_>,
        name: String,
        #[graphql(default_with = "TokenScope::Ingest")] scope: TokenScope,
    ) -> Result<CreatedApiToken> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(user_error("토큰 이름을 입력하세요"));
        }
        let token = generate_token();
        let saved = api_token::ActiveModel {
            name: Set(name),
            token_hash: Set(sha256_hex(&token)),
            token_prefix: Set(token.chars().take(10).collect()),
            scope: Set(scope),
            created_at: Set(Utc::now()),
            last_used_at: Set(None),
            ..Default::default()
        }
        .insert(db(ctx))
        .await?;
        Ok(CreatedApiToken { token, api_token: saved })
    }

    async fn delete_api_token(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        Ok(api_token::Entity::delete_by_id(id).exec(db(ctx)).await?.rows_affected > 0)
    }
}
