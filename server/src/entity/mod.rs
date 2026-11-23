//! SeaORM 엔티티. 테이블 하나당 파일 하나.
//! 스키마 변경은 `db::migration`에 새 마이그레이션을 추가하고 여기 정의도 함께 맞춘다.
//!
//! 계정별 데이터(`user_id` 컬럼이 있는 테이블)는 [`UserOwned`]를 구현하고, 읽고 고칠 때 항상 [`OfUser::of_user`]로
//! 범위를 건다. 새로 저장할 때 `user_id`를 빼먹으면 `before_save`가 거절한다 (`insert_many`는 검사하지 않는다).

pub mod account;
pub mod account_balance;
pub mod api_token;
pub mod card_benefit;
pub mod card_product;
pub mod card_tier;
pub mod category;
pub mod enums;
pub mod ledger;
pub mod payment_method;
pub mod plan_item;
pub mod recurring_rule;
pub mod recurring_rule_benefit;
pub mod transaction;
pub mod transaction_benefit;
pub mod user;

use sea_orm::{ColumnTrait, DeleteMany, EntityTrait, QueryFilter, Select, UpdateMany};

/// 계정마다 따로 갖는 테이블
pub trait UserOwned: EntityTrait {
    fn user_column() -> Self::Column;
}

/// 조회·수정·삭제를 한 계정의 행으로 한정한다.
pub trait OfUser {
    fn of_user(self, user_id: i32) -> Self;
}

impl<E: UserOwned> OfUser for Select<E> {
    fn of_user(self, user_id: i32) -> Self {
        self.filter(E::user_column().eq(user_id))
    }
}

impl<E: UserOwned> OfUser for UpdateMany<E> {
    fn of_user(self, user_id: i32) -> Self {
        self.filter(E::user_column().eq(user_id))
    }
}

impl<E: UserOwned> OfUser for DeleteMany<E> {
    fn of_user(self, user_id: i32) -> Self {
        self.filter(E::user_column().eq(user_id))
    }
}

/// `user_id` 컬럼이 있는 엔티티에 붙인다: [`UserOwned`] 구현과, 계정 없이 새 행을 저장하지 못하게 막는 `before_save`.
macro_rules! user_owned {
    () => {
        impl $crate::entity::UserOwned for Entity {
            fn user_column() -> Column {
                Column::UserId
            }
        }

        #[sea_orm::prelude::async_trait::async_trait]
        impl ActiveModelBehavior for ActiveModel {
            async fn before_save<C>(self, _db: &C, insert: bool) -> Result<Self, DbErr>
            where
                C: ConnectionTrait,
            {
                let missing = match &self.user_id {
                    sea_orm::ActiveValue::Set(id) | sea_orm::ActiveValue::Unchanged(id) => *id <= 0,
                    sea_orm::ActiveValue::NotSet => insert,
                };
                if missing {
                    return Err(DbErr::Custom(concat!(module_path!(), ": user_id 없이 저장할 수 없습니다").into()));
                }
                Ok(self)
            }
        }
    };
}
pub(crate) use user_owned;
