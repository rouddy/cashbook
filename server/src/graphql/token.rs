//! API 토큰 발급/폐기(토큰은 자동 입력과 GraphQL을 모두 쓴다)와 MCP로 연결된 앱(OAuth 연결) 목록·끊기.

use async_graphql::{Context, Object, Result, SimpleObject};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};

use super::{db, uid, user_error};
use crate::auth::{generate_token, sha256_hex};
use crate::entity::{OfUser, api_token, oauth_grant};

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
        let uid = uid(ctx)?;
        Ok(api_token::Entity::find().of_user(uid).order_by_desc(api_token::Column::Id).all(db(ctx)).await?)
    }

    /// MCP로 연결된 앱 (OpenAI Dots·ChatGPT 플러그인 등이 OAuth로 승인받은 연결)
    async fn connected_apps(&self, ctx: &Context<'_>) -> Result<Vec<oauth_grant::Model>> {
        let uid = uid(ctx)?;
        Ok(oauth_grant::Entity::find().of_user(uid).order_by_desc(oauth_grant::Column::Id).all(db(ctx)).await?)
    }
}

#[derive(Default)]
pub struct TokenMutation;

#[Object]
impl TokenMutation {
    /// 토큰 발급 (예: "아이폰 단축어", "가계부 앱"). 자동 입력과 GraphQL 전체를 쓸 수 있다.
    async fn create_api_token(&self, ctx: &Context<'_>, name: String) -> Result<CreatedApiToken> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(user_error("토큰 이름을 입력하세요"));
        }
        let token = generate_token();
        let saved = api_token::ActiveModel {
            user_id: Set(uid(ctx)?),
            name: Set(name),
            token_hash: Set(sha256_hex(&token)),
            token_prefix: Set(token.chars().take(10).collect()),
            created_at: Set(Utc::now()),
            last_used_at: Set(None),
            ..Default::default()
        }
        .insert(db(ctx))
        .await?;
        Ok(CreatedApiToken { token, api_token: saved })
    }

    async fn delete_api_token(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let uid = uid(ctx)?;
        Ok(api_token::Entity::delete_many().of_user(uid).filter(api_token::Column::Id.eq(id)).exec(db(ctx)).await?.rows_affected > 0)
    }

    /// 연결 끊기: 그 앱의 액세스·리프레시 토큰이 바로 무효가 된다
    async fn revoke_connected_app(&self, ctx: &Context<'_>, id: i32) -> Result<bool> {
        let uid = uid(ctx)?;
        Ok(oauth_grant::Entity::delete_many().of_user(uid).filter(oauth_grant::Column::Id.eq(id)).exec(db(ctx)).await?.rows_affected > 0)
    }
}
