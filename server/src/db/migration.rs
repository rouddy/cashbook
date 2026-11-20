//! 손으로 쓰는 마이그레이션. 엔티티(`crate::entity`)와 컬럼 타입을 맞춰야 한다.
//! 스키마를 바꿀 때는 기존 마이그레이션을 고치지 말고 새 마이그레이션을 추가한다.

use sea_orm_migration::{prelude::*, schema::*};

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261023_000001_init::Migration),
            Box::new(m20261023_000002_api_token_scope::Migration),
            Box::new(m20261102_000003_card_benefit_kind_stackable::Migration),
            Box::new(m20261109_000004_mysql_text_columns::Migration),
            Box::new(m20261119_000005_transaction_split::Migration),
            Box::new(m20261120_000006_ledger_transfers::Migration),
            Box::new(m20261120_000007_convert_legacy_plan::Migration),
            Box::new(m20261120_000008_drop_excluded_from_spending::Migration),
            Box::new(m20261120_000009_recurring_match_amount::Migration),
            Box::new(m20261120_000010_recurring_rule_benefits::Migration),
        ]
    }
}

mod m20261023_000001_init {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261023_000001_init"
        }
    }

    #[derive(DeriveIden)]
    enum Ledgers {
        Table,
        Id,
        Name,
        Description,
        IsDefault,
        SortOrder,
        CreatedAt,
    }

    #[derive(DeriveIden)]
    enum Categories {
        Table,
        Id,
        LedgerId,
        Name,
        Kind,
        MonthlyBudget,
        IsFixed,
        ExcludedFromSpending,
        Color,
        SortOrder,
        Archived,
    }

    #[derive(DeriveIden)]
    enum Accounts {
        Table,
        Id,
        Name,
        Bank,
        Kind,
        AccountNumber,
        Balance,
        BalanceDate,
        IncludeInAvailable,
        InterestRate,
        MaturityDate,
        Memo,
        SortOrder,
        Archived,
        CreatedAt,
        UpdatedAt,
    }

    #[derive(DeriveIden)]
    enum AccountBalances {
        Table,
        Id,
        AccountId,
        Balance,
        BalanceDate,
        Memo,
        CreatedAt,
    }

    #[derive(DeriveIden)]
    enum PaymentMethods {
        Table,
        Id,
        Name,
        Kind,
        Issuer,
        ProductName,
        Last4,
        Aliases,
        AccountId,
        PaymentDay,
        ClosingDay,
        ClosingMonthOffset,
        MonthlyTarget,
        AnnualFee,
        ExpiresOn,
        Color,
        Memo,
        SortOrder,
        Archived,
        CreatedAt,
    }

    #[derive(DeriveIden)]
    enum CardBenefits {
        Table,
        Id,
        PaymentMethodId,
        Name,
        Rate,
        MinAmount,
        PerTxMax,
        MonthlyCap,
        SharesCardLimit,
        ExcludesPerformance,
        SortOrder,
    }

    #[derive(DeriveIden)]
    enum CardTiers {
        Table,
        Id,
        PaymentMethodId,
        MinSpending,
        BenefitLimit,
    }

    #[derive(DeriveIden)]
    enum RecurringRules {
        Table,
        Id,
        LedgerId,
        Description,
        Amount,
        CategoryId,
        PaymentMethodId,
        DayOfMonth,
        IntervalMonths,
        StartMonth,
        EndMonth,
        Member,
        Memo,
        MatchKeyword,
        ExcludeFromPerformance,
        Active,
        CreatedAt,
        UpdatedAt,
    }

    #[derive(DeriveIden)]
    enum Transactions {
        Table,
        Id,
        LedgerId,
        Date,
        Amount,
        CategoryId,
        PaymentMethodId,
        Description,
        Memo,
        Member,
        Status,
        InstallmentMonths,
        ExcludeFromPerformance,
        RecurringRuleId,
        RecurringMonth,
        Source,
        ExternalId,
        RawText,
        CreatedAt,
        UpdatedAt,
    }

    #[derive(DeriveIden)]
    enum TransactionBenefits {
        Table,
        TransactionId,
        CardBenefitId,
    }

    #[derive(DeriveIden)]
    enum PlanItems {
        Table,
        Id,
        LedgerId,
        Kind,
        Name,
        Amount,
        Member,
        SortOrder,
    }

    #[derive(DeriveIden)]
    enum ApiTokens {
        Table,
        Id,
        Name,
        TokenHash,
        TokenPrefix,
        CreatedAt,
        LastUsedAt,
    }

    fn fk(
        name: &str,
        from: (impl IntoIden, impl IntoIden),
        to: (impl IntoIden, impl IntoIden),
        on_delete: ForeignKeyAction,
    ) -> ForeignKeyCreateStatement {
        ForeignKey::create()
            .name(name)
            .from(from.0, from.1)
            .to(to.0, to.1)
            .on_delete(on_delete)
            .on_update(ForeignKeyAction::Cascade)
            .to_owned()
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(Ledgers::Table)
                        .col(pk_auto(Ledgers::Id))
                        .col(string_uniq(Ledgers::Name))
                        .col(string_null(Ledgers::Description))
                        .col(boolean(Ledgers::IsDefault))
                        .col(integer(Ledgers::SortOrder))
                        .col(timestamp_with_time_zone(Ledgers::CreatedAt))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(Categories::Table)
                        .col(pk_auto(Categories::Id))
                        .col(integer(Categories::LedgerId))
                        .col(string(Categories::Name))
                        .col(string(Categories::Kind))
                        .col(big_integer_null(Categories::MonthlyBudget))
                        .col(boolean(Categories::IsFixed))
                        .col(boolean(Categories::ExcludedFromSpending))
                        .col(string_null(Categories::Color))
                        .col(integer(Categories::SortOrder))
                        .col(boolean(Categories::Archived))
                        .foreign_key(&mut fk(
                            "fk_categories_ledger",
                            (Categories::Table, Categories::LedgerId),
                            (Ledgers::Table, Ledgers::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name("uq_categories_ledger_name")
                        .table(Categories::Table)
                        .col(Categories::LedgerId)
                        .col(Categories::Name)
                        .unique()
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(Accounts::Table)
                        .col(pk_auto(Accounts::Id))
                        .col(string(Accounts::Name))
                        .col(string_null(Accounts::Bank))
                        .col(string(Accounts::Kind))
                        .col(string_null(Accounts::AccountNumber))
                        .col(big_integer(Accounts::Balance))
                        .col(date_null(Accounts::BalanceDate))
                        .col(boolean(Accounts::IncludeInAvailable))
                        .col(double_null(Accounts::InterestRate))
                        .col(date_null(Accounts::MaturityDate))
                        .col(string_null(Accounts::Memo))
                        .col(integer(Accounts::SortOrder))
                        .col(boolean(Accounts::Archived))
                        .col(timestamp_with_time_zone(Accounts::CreatedAt))
                        .col(timestamp_with_time_zone(Accounts::UpdatedAt))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(AccountBalances::Table)
                        .col(pk_auto(AccountBalances::Id))
                        .col(integer(AccountBalances::AccountId))
                        .col(big_integer(AccountBalances::Balance))
                        .col(date(AccountBalances::BalanceDate))
                        .col(string_null(AccountBalances::Memo))
                        .col(timestamp_with_time_zone(AccountBalances::CreatedAt))
                        .foreign_key(&mut fk(
                            "fk_account_balances_account",
                            (AccountBalances::Table, AccountBalances::AccountId),
                            (Accounts::Table, Accounts::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name("ix_account_balances_account_date")
                        .table(AccountBalances::Table)
                        .col(AccountBalances::AccountId)
                        .col(AccountBalances::BalanceDate)
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(PaymentMethods::Table)
                        .col(pk_auto(PaymentMethods::Id))
                        .col(string_uniq(PaymentMethods::Name))
                        .col(string(PaymentMethods::Kind))
                        .col(string_null(PaymentMethods::Issuer))
                        .col(string_null(PaymentMethods::ProductName))
                        .col(string_null(PaymentMethods::Last4))
                        .col(string_null(PaymentMethods::Aliases))
                        .col(integer_null(PaymentMethods::AccountId))
                        .col(integer_null(PaymentMethods::PaymentDay))
                        .col(integer_null(PaymentMethods::ClosingDay))
                        .col(integer_null(PaymentMethods::ClosingMonthOffset))
                        .col(big_integer_null(PaymentMethods::MonthlyTarget))
                        .col(big_integer_null(PaymentMethods::AnnualFee))
                        .col(string_null(PaymentMethods::ExpiresOn))
                        .col(string_null(PaymentMethods::Color))
                        .col(string_null(PaymentMethods::Memo))
                        .col(integer(PaymentMethods::SortOrder))
                        .col(boolean(PaymentMethods::Archived))
                        .col(timestamp_with_time_zone(PaymentMethods::CreatedAt))
                        .foreign_key(&mut fk(
                            "fk_payment_methods_account",
                            (PaymentMethods::Table, PaymentMethods::AccountId),
                            (Accounts::Table, Accounts::Id),
                            ForeignKeyAction::SetNull,
                        ))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(CardBenefits::Table)
                        .col(pk_auto(CardBenefits::Id))
                        .col(integer(CardBenefits::PaymentMethodId))
                        .col(string(CardBenefits::Name))
                        .col(double(CardBenefits::Rate))
                        .col(big_integer_null(CardBenefits::MinAmount))
                        .col(big_integer_null(CardBenefits::PerTxMax))
                        .col(big_integer_null(CardBenefits::MonthlyCap))
                        .col(boolean(CardBenefits::SharesCardLimit))
                        .col(boolean(CardBenefits::ExcludesPerformance))
                        .col(integer(CardBenefits::SortOrder))
                        .foreign_key(&mut fk(
                            "fk_card_benefits_payment_method",
                            (CardBenefits::Table, CardBenefits::PaymentMethodId),
                            (PaymentMethods::Table, PaymentMethods::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(CardTiers::Table)
                        .col(pk_auto(CardTiers::Id))
                        .col(integer(CardTiers::PaymentMethodId))
                        .col(big_integer(CardTiers::MinSpending))
                        .col(big_integer(CardTiers::BenefitLimit))
                        .foreign_key(&mut fk(
                            "fk_card_tiers_payment_method",
                            (CardTiers::Table, CardTiers::PaymentMethodId),
                            (PaymentMethods::Table, PaymentMethods::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(RecurringRules::Table)
                        .col(pk_auto(RecurringRules::Id))
                        .col(integer(RecurringRules::LedgerId))
                        .col(string(RecurringRules::Description))
                        .col(big_integer(RecurringRules::Amount))
                        .col(integer_null(RecurringRules::CategoryId))
                        .col(integer_null(RecurringRules::PaymentMethodId))
                        .col(integer(RecurringRules::DayOfMonth))
                        .col(integer(RecurringRules::IntervalMonths))
                        .col(string(RecurringRules::StartMonth))
                        .col(string_null(RecurringRules::EndMonth))
                        .col(string_null(RecurringRules::Member))
                        .col(string_null(RecurringRules::Memo))
                        .col(string_null(RecurringRules::MatchKeyword))
                        .col(boolean(RecurringRules::ExcludeFromPerformance))
                        .col(boolean(RecurringRules::Active))
                        .col(timestamp_with_time_zone(RecurringRules::CreatedAt))
                        .col(timestamp_with_time_zone(RecurringRules::UpdatedAt))
                        .foreign_key(&mut fk(
                            "fk_recurring_rules_ledger",
                            (RecurringRules::Table, RecurringRules::LedgerId),
                            (Ledgers::Table, Ledgers::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .foreign_key(&mut fk(
                            "fk_recurring_rules_category",
                            (RecurringRules::Table, RecurringRules::CategoryId),
                            (Categories::Table, Categories::Id),
                            ForeignKeyAction::SetNull,
                        ))
                        .foreign_key(&mut fk(
                            "fk_recurring_rules_payment_method",
                            (RecurringRules::Table, RecurringRules::PaymentMethodId),
                            (PaymentMethods::Table, PaymentMethods::Id),
                            ForeignKeyAction::SetNull,
                        ))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(Transactions::Table)
                        .col(pk_auto(Transactions::Id))
                        .col(integer(Transactions::LedgerId))
                        .col(date(Transactions::Date))
                        .col(big_integer(Transactions::Amount))
                        .col(integer_null(Transactions::CategoryId))
                        .col(integer_null(Transactions::PaymentMethodId))
                        .col(string(Transactions::Description))
                        .col(string_null(Transactions::Memo))
                        .col(string_null(Transactions::Member))
                        .col(string(Transactions::Status))
                        .col(integer(Transactions::InstallmentMonths))
                        .col(boolean(Transactions::ExcludeFromPerformance))
                        .col(integer_null(Transactions::RecurringRuleId))
                        .col(string_null(Transactions::RecurringMonth))
                        .col(string(Transactions::Source))
                        .col(string_null(Transactions::ExternalId))
                        .col(string_null(Transactions::RawText))
                        .col(timestamp_with_time_zone(Transactions::CreatedAt))
                        .col(timestamp_with_time_zone(Transactions::UpdatedAt))
                        .foreign_key(&mut fk(
                            "fk_transactions_ledger",
                            (Transactions::Table, Transactions::LedgerId),
                            (Ledgers::Table, Ledgers::Id),
                            ForeignKeyAction::Restrict,
                        ))
                        .foreign_key(&mut fk(
                            "fk_transactions_category",
                            (Transactions::Table, Transactions::CategoryId),
                            (Categories::Table, Categories::Id),
                            ForeignKeyAction::SetNull,
                        ))
                        .foreign_key(&mut fk(
                            "fk_transactions_payment_method",
                            (Transactions::Table, Transactions::PaymentMethodId),
                            (PaymentMethods::Table, PaymentMethods::Id),
                            ForeignKeyAction::SetNull,
                        ))
                        .foreign_key(&mut fk(
                            "fk_transactions_recurring_rule",
                            (Transactions::Table, Transactions::RecurringRuleId),
                            (RecurringRules::Table, RecurringRules::Id),
                            ForeignKeyAction::SetNull,
                        ))
                        .to_owned(),
                )
                .await?;
            for (name, cols, unique) in [
                ("ix_transactions_date", vec![Transactions::Date], false),
                ("ix_transactions_ledger_date", vec![Transactions::LedgerId, Transactions::Date], false),
                (
                    "ix_transactions_payment_method_date",
                    vec![Transactions::PaymentMethodId, Transactions::Date],
                    false,
                ),
                ("ix_transactions_status_date", vec![Transactions::Status, Transactions::Date], false),
                ("ix_transactions_description", vec![Transactions::Description], false),
                ("uq_transactions_external_id", vec![Transactions::ExternalId], true),
                (
                    "uq_transactions_recurring_occurrence",
                    vec![Transactions::RecurringRuleId, Transactions::RecurringMonth],
                    true,
                ),
            ] {
                let mut index = Index::create();
                index.name(name).table(Transactions::Table);
                for col in cols {
                    index.col(col);
                }
                if unique {
                    index.unique();
                }
                manager.create_index(index.to_owned()).await?;
            }

            manager
                .create_table(
                    Table::create()
                        .table(TransactionBenefits::Table)
                        .col(integer(TransactionBenefits::TransactionId))
                        .col(integer(TransactionBenefits::CardBenefitId))
                        .primary_key(
                            Index::create()
                                .col(TransactionBenefits::TransactionId)
                                .col(TransactionBenefits::CardBenefitId),
                        )
                        .foreign_key(&mut fk(
                            "fk_transaction_benefits_transaction",
                            (TransactionBenefits::Table, TransactionBenefits::TransactionId),
                            (Transactions::Table, Transactions::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .foreign_key(&mut fk(
                            "fk_transaction_benefits_card_benefit",
                            (TransactionBenefits::Table, TransactionBenefits::CardBenefitId),
                            (CardBenefits::Table, CardBenefits::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name("ix_transaction_benefits_card_benefit")
                        .table(TransactionBenefits::Table)
                        .col(TransactionBenefits::CardBenefitId)
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(PlanItems::Table)
                        .col(pk_auto(PlanItems::Id))
                        .col(integer(PlanItems::LedgerId))
                        .col(string(PlanItems::Kind))
                        .col(string(PlanItems::Name))
                        .col(big_integer(PlanItems::Amount))
                        .col(string_null(PlanItems::Member))
                        .col(integer(PlanItems::SortOrder))
                        .foreign_key(&mut fk(
                            "fk_plan_items_ledger",
                            (PlanItems::Table, PlanItems::LedgerId),
                            (Ledgers::Table, Ledgers::Id),
                            ForeignKeyAction::Cascade,
                        ))
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(ApiTokens::Table)
                        .col(pk_auto(ApiTokens::Id))
                        .col(string(ApiTokens::Name))
                        .col(string_uniq(ApiTokens::TokenHash))
                        .col(string(ApiTokens::TokenPrefix))
                        .col(timestamp_with_time_zone(ApiTokens::CreatedAt))
                        .col(timestamp_with_time_zone_null(ApiTokens::LastUsedAt))
                        .to_owned(),
                )
                .await?;

            Ok(())
        }
    }
}

/// API 토큰 권한 범위. 기존 토큰은 자동 입력 전용이 된다.
mod m20261023_000002_api_token_scope {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261023_000002_api_token_scope"
        }
    }

    #[derive(DeriveIden)]
    enum ApiTokens {
        Table,
        Scope,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(ApiTokens::Table)
                        .add_column(string(ApiTokens::Scope).default("INGEST"))
                        .to_owned(),
                )
                .await
        }
    }
}

/// 카드 혜택 종류(할인/포인트/마일리지)와 중복 적용 여부. 기존 혜택은 할인, 중복 적용 아님.
mod m20261102_000003_card_benefit_kind_stackable {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261102_000003_card_benefit_kind_stackable"
        }
    }

    #[derive(DeriveIden)]
    enum CardBenefits {
        Table,
        Kind,
        Stackable,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            // SQLite는 ALTER TABLE 한 번에 컬럼 하나만 추가할 수 있다.
            manager
                .alter_table(
                    Table::alter()
                        .table(CardBenefits::Table)
                        .add_column(string(CardBenefits::Kind).default("DISCOUNT"))
                        .to_owned(),
                )
                .await?;
            manager
                .alter_table(
                    Table::alter()
                        .table(CardBenefits::Table)
                        .add_column(boolean(CardBenefits::Stackable).default(false))
                        .to_owned(),
                )
                .await
        }
    }
}

/// MySQL에서 `string()`은 varchar(255)라 긴 자동 입력 원문·메모가 들어가지 않는다. 긴 글 컬럼을 TEXT로 넓힌다.
/// SQLite는 길이를 따지지 않으므로 아무것도 하지 않는다. (인덱스가 걸린 description·external_id는 그대로)
mod m20261109_000004_mysql_text_columns {
    use super::*;
    use sea_orm::DbBackend;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261109_000004_mysql_text_columns"
        }
    }

    #[derive(DeriveIden)]
    enum Ledgers {
        Table,
        Description,
    }

    #[derive(DeriveIden)]
    enum Accounts {
        Table,
        Memo,
    }

    #[derive(DeriveIden)]
    enum AccountBalances {
        Table,
        Memo,
    }

    #[derive(DeriveIden)]
    enum PaymentMethods {
        Table,
        Aliases,
        Memo,
    }

    #[derive(DeriveIden)]
    enum RecurringRules {
        Table,
        Memo,
        MatchKeyword,
    }

    #[derive(DeriveIden)]
    enum Transactions {
        Table,
        Memo,
        RawText,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            if manager.get_database_backend() != DbBackend::MySql {
                return Ok(());
            }
            let columns: [(DynIden, DynIden); 9] = [
                (Ledgers::Table.into_iden(), Ledgers::Description.into_iden()),
                (Accounts::Table.into_iden(), Accounts::Memo.into_iden()),
                (AccountBalances::Table.into_iden(), AccountBalances::Memo.into_iden()),
                (PaymentMethods::Table.into_iden(), PaymentMethods::Aliases.into_iden()),
                (PaymentMethods::Table.into_iden(), PaymentMethods::Memo.into_iden()),
                (RecurringRules::Table.into_iden(), RecurringRules::Memo.into_iden()),
                (RecurringRules::Table.into_iden(), RecurringRules::MatchKeyword.into_iden()),
                (Transactions::Table.into_iden(), Transactions::Memo.into_iden()),
                (Transactions::Table.into_iden(), Transactions::RawText.into_iden()),
            ];
            for (table, column) in columns {
                manager
                    .alter_table(Table::alter().table(table).modify_column(text_null(column)).to_owned())
                    .await?;
            }
            Ok(())
        }
    }
}

/// 거래 분할: 한 결제를 여러 장부에 나눠 담을 때, 나눠 옮긴 거래(자식)가 원래 거래(부모)를 가리킨다.
/// SQLite는 ALTER TABLE로 외래 키를 붙일 수 없어 인덱스만 둔다 (부모를 지우는 경로는 분할을 먼저 합친다).
mod m20261119_000005_transaction_split {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261119_000005_transaction_split"
        }
    }

    #[derive(DeriveIden)]
    enum Transactions {
        Table,
        SplitParentId,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(Transactions::Table)
                        .add_column(integer_null(Transactions::SplitParentId))
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name("ix_transactions_split_parent")
                        .table(Transactions::Table)
                        .col(Transactions::SplitParentId)
                        .to_owned(),
                )
                .await
        }
    }
}

/// 장부 이체: 거래·반복 규칙이 금액을 다른 장부로 옮길 때 받는 장부.
/// SQLite는 ALTER TABLE로 외래 키를 붙일 수 없어 인덱스만 둔다 (장부를 지울 때 이체가 있는지 먼저 확인한다).
mod m20261120_000006_ledger_transfers {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261120_000006_ledger_transfers"
        }
    }

    #[derive(DeriveIden)]
    enum Transactions {
        Table,
        TransferLedgerId,
        Date,
    }

    #[derive(DeriveIden)]
    enum RecurringRules {
        Table,
        TransferLedgerId,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(Transactions::Table)
                        .add_column(integer_null(Transactions::TransferLedgerId))
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name("ix_transactions_transfer_ledger_date")
                        .table(Transactions::Table)
                        .col(Transactions::TransferLedgerId)
                        .col(Transactions::Date)
                        .to_owned(),
                )
                .await?;
            manager
                .alter_table(
                    Table::alter()
                        .table(RecurringRules::Table)
                        .add_column(integer_null(RecurringRules::TransferLedgerId))
                        .to_owned(),
                )
                .await
        }
    }
}

/// 예전 지출계획을 새 방식으로 옮긴다 (규칙은 `domain::legacy_plan`).
/// - '합계 제외' 카테고리(경조사비·여행) → 같은 이름의 장부로 카테고리·거래·반복 규칙째
/// - 배분(재홍 용돈·경조사비·여행적금) → 맞는 장부로 매월 1일 이체 반복, 월 수입(월급) → 정기수익 항목 매월 10일 반복 수입
/// 반복 규칙은 이번 달부터 시작하고, 예정 거래는 서버가 시작하면서 만든다. 데이터만 바꾸므로 한 트랜잭션으로 돌린다.
mod m20261120_000007_convert_legacy_plan {
    use super::*;
    use chrono::{DateTime, Utc};
    use sea_orm::{ConnectionTrait, QueryResult};

    use crate::domain::dates::{YearMonth, today_in};
    use crate::domain::legacy_plan::{self, Conversion, LedgerInfo, LegacyItem, LegacyKind};

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261120_000007_convert_legacy_plan"
        }
    }

    #[derive(DeriveIden)]
    enum Ledgers {
        Table,
        Id,
        Name,
        Description,
        IsDefault,
        SortOrder,
        CreatedAt,
    }

    #[derive(DeriveIden)]
    enum Categories {
        Table,
        Id,
        LedgerId,
        Name,
        Kind,
        MonthlyBudget,
        IsFixed,
        ExcludedFromSpending,
        Color,
        SortOrder,
        Archived,
    }

    #[derive(DeriveIden)]
    enum PlanItems {
        Table,
        Id,
        LedgerId,
        Kind,
        Name,
        Amount,
        SortOrder,
    }

    #[derive(DeriveIden)]
    enum RecurringRules {
        Table,
        LedgerId,
        Description,
        Amount,
        CategoryId,
        PaymentMethodId,
        DayOfMonth,
        IntervalMonths,
        StartMonth,
        EndMonth,
        Member,
        Memo,
        MatchKeyword,
        ExcludeFromPerformance,
        TransferLedgerId,
        Active,
        CreatedAt,
        UpdatedAt,
    }

    #[derive(DeriveIden)]
    enum Transactions {
        Table,
        LedgerId,
        CategoryId,
    }

    fn get<T: sea_orm::TryGetable>(row: &QueryResult, col: &str) -> Result<T, DbErr> {
        row.try_get("", col)
    }

    /// 다음 정렬 순서: `sort_order`만 고르는 쿼리 결과의 최댓값 + 1
    async fn next_sort_order<C: ConnectionTrait>(db: &C, q: SelectStatement) -> Result<i32, DbErr> {
        let mut max = -1;
        for r in db.query_all(&q).await? {
            max = std::cmp::max(max, get::<i32>(&r, "sort_order")?);
        }
        Ok(max + 1)
    }

    async fn find_or_create_ledger<C: ConnectionTrait>(db: &C, name: &str, now: DateTime<Utc>) -> Result<i32, DbErr> {
        let found = db
            .query_one(&Query::select().column(Ledgers::Id).from(Ledgers::Table).and_where(Expr::col(Ledgers::Name).eq(name)).to_owned())
            .await?;
        if let Some(row) = found {
            return get(&row, "id");
        }
        let order = next_sort_order(db, Query::select().column(Ledgers::SortOrder).from(Ledgers::Table).to_owned()).await?;
        let res = db
            .execute(
                &Query::insert()
                    .into_table(Ledgers::Table)
                    .columns([Ledgers::Name, Ledgers::Description, Ledgers::IsDefault, Ledgers::SortOrder, Ledgers::CreatedAt])
                    .values_panic([name.into(), Option::<String>::None.into(), false.into(), order.into(), now.into()])
                    .to_owned(),
            )
            .await?;
        Ok(res.last_insert_id() as i32)
    }

    async fn find_category<C: ConnectionTrait>(db: &C, ledger_id: i32, name: &str) -> Result<Option<(i32, bool)>, DbErr> {
        let row = db
            .query_one(
                &Query::select()
                    .columns([Categories::Id, Categories::Archived])
                    .from(Categories::Table)
                    .and_where(Expr::col(Categories::LedgerId).eq(ledger_id))
                    .and_where(Expr::col(Categories::Name).eq(name))
                    .to_owned(),
            )
            .await?;
        row.map(|r| Ok((get(&r, "id")?, get(&r, "archived")?))).transpose()
    }

    fn category_sort_orders(ledger_id: i32) -> SelectStatement {
        Query::select()
            .column(Categories::SortOrder)
            .from(Categories::Table)
            .and_where(Expr::col(Categories::LedgerId).eq(ledger_id))
            .to_owned()
    }

    /// 합계 제외 카테고리 하나를 같은 이름의 장부로 옮긴다. 그 장부에 같은 이름 카테고리가 있으면 그쪽으로 합친다.
    async fn move_excluded_category<C: ConnectionTrait>(db: &C, id: i32, ledger_id: i32, name: &str, now: DateTime<Utc>) -> Result<(), DbErr> {
        let target = find_or_create_ledger(db, name, now).await?;
        if target == ledger_id {
            return Ok(());
        }
        let category_id = match find_category(db, target, name).await? {
            Some((existing, _)) => existing,
            None => {
                let order = next_sort_order(db, category_sort_orders(target)).await?;
                db.execute(
                    &Query::update()
                        .table(Categories::Table)
                        .values([(Categories::LedgerId, target.into()), (Categories::SortOrder, order.into())])
                        .and_where(Expr::col(Categories::Id).eq(id))
                        .to_owned(),
                )
                .await?;
                id
            }
        };
        db.execute(
            &Query::update()
                .table(Transactions::Table)
                .values([(Transactions::LedgerId, target.into()), (Transactions::CategoryId, category_id.into())])
                .and_where(Expr::col(Transactions::CategoryId).eq(id))
                .to_owned(),
        )
        .await?;
        db.execute(
            &Query::update()
                .table(RecurringRules::Table)
                .values([(RecurringRules::LedgerId, target.into()), (RecurringRules::CategoryId, category_id.into())])
                .and_where(Expr::col(RecurringRules::CategoryId).eq(id))
                .to_owned(),
        )
        .await?;
        if category_id != id {
            db.execute(&Query::delete().from_table(Categories::Table).and_where(Expr::col(Categories::Id).eq(id)).to_owned())
                .await?;
        }
        Ok(())
    }

    /// 장부의 정기수익 수입 항목 (없으면 만들고, 보관돼 있으면 되살린다)
    async fn regular_income_category<C: ConnectionTrait>(db: &C, ledger_id: i32) -> Result<i32, DbErr> {
        let name = legacy_plan::REGULAR_INCOME_CATEGORY;
        if let Some((id, archived)) = find_category(db, ledger_id, name).await? {
            if archived {
                db.execute(
                    &Query::update()
                        .table(Categories::Table)
                        .value(Categories::Archived, false)
                        .and_where(Expr::col(Categories::Id).eq(id))
                        .to_owned(),
                )
                .await?;
            }
            return Ok(id);
        }
        let order = next_sort_order(db, category_sort_orders(ledger_id)).await?;
        let res = db
            .execute(
                &Query::insert()
                    .into_table(Categories::Table)
                    .columns([
                        Categories::LedgerId,
                        Categories::Name,
                        Categories::Kind,
                        Categories::MonthlyBudget,
                        Categories::IsFixed,
                        Categories::ExcludedFromSpending,
                        Categories::Color,
                        Categories::SortOrder,
                        Categories::Archived,
                    ])
                    .values_panic([
                        ledger_id.into(),
                        name.into(),
                        "INCOME".into(),
                        Option::<i64>::None.into(),
                        false.into(),
                        false.into(),
                        Option::<String>::None.into(),
                        order.into(),
                        false.into(),
                    ])
                    .to_owned(),
            )
            .await?;
        Ok(res.last_insert_id() as i32)
    }

    struct NewRule<'a> {
        ledger_id: i32,
        description: &'a str,
        amount: i64,
        category_id: Option<i32>,
        transfer_ledger_id: Option<i32>,
        day_of_month: i32,
    }

    async fn insert_rule<C: ConnectionTrait>(db: &C, rule: NewRule<'_>, start_month: &str, now: DateTime<Utc>) -> Result<(), DbErr> {
        db.execute(
            &Query::insert()
                .into_table(RecurringRules::Table)
                .columns([
                    RecurringRules::LedgerId,
                    RecurringRules::Description,
                    RecurringRules::Amount,
                    RecurringRules::CategoryId,
                    RecurringRules::PaymentMethodId,
                    RecurringRules::DayOfMonth,
                    RecurringRules::IntervalMonths,
                    RecurringRules::StartMonth,
                    RecurringRules::EndMonth,
                    RecurringRules::Member,
                    RecurringRules::Memo,
                    RecurringRules::MatchKeyword,
                    RecurringRules::ExcludeFromPerformance,
                    RecurringRules::TransferLedgerId,
                    RecurringRules::Active,
                    RecurringRules::CreatedAt,
                    RecurringRules::UpdatedAt,
                ])
                .values_panic([
                    rule.ledger_id.into(),
                    rule.description.into(),
                    rule.amount.into(),
                    rule.category_id.into(),
                    Option::<i32>::None.into(),
                    rule.day_of_month.into(),
                    1.into(),
                    start_month.into(),
                    Option::<String>::None.into(),
                    Option::<String>::None.into(),
                    Option::<String>::None.into(),
                    Option::<String>::None.into(),
                    false.into(),
                    rule.transfer_ledger_id.into(),
                    true.into(),
                    now.into(),
                    now.into(),
                ])
                .to_owned(),
        )
        .await?;
        Ok(())
    }

    /// 반복 규칙 시작 월: 서버와 같은 시간대(`CASHBOOK_TZ`, 기본 Asia/Seoul)의 이번 달
    pub(super) fn this_month() -> String {
        let tz = std::env::var("CASHBOOK_TZ")
            .ok()
            .and_then(|v| v.parse::<chrono_tz::Tz>().ok())
            .unwrap_or(chrono_tz::Asia::Seoul);
        YearMonth::of(today_in(tz)).to_string()
    }

    pub(super) async fn convert<C: ConnectionTrait>(db: &C, start_month: &str) -> Result<(), DbErr> {
        let now = Utc::now();
        let excluded = db
            .query_all(
                &Query::select()
                    .columns([Categories::Id, Categories::LedgerId, Categories::Name])
                    .from(Categories::Table)
                    .and_where(Expr::col(Categories::ExcludedFromSpending).eq(true))
                    .order_by(Categories::LedgerId, Order::Asc)
                    .order_by(Categories::SortOrder, Order::Asc)
                    .order_by(Categories::Id, Order::Asc)
                    .to_owned(),
            )
            .await?;
        for row in &excluded {
            let name: String = get(row, "name")?;
            move_excluded_category(db, get(row, "id")?, get(row, "ledger_id")?, &name, now).await?;
        }

        let ledgers = db
            .query_all(
                &Query::select()
                    .columns([Ledgers::Id, Ledgers::Name, Ledgers::Description])
                    .from(Ledgers::Table)
                    .order_by(Ledgers::SortOrder, Order::Asc)
                    .order_by(Ledgers::Id, Order::Asc)
                    .to_owned(),
            )
            .await?
            .iter()
            .map(|r| Ok(LedgerInfo { id: get(r, "id")?, name: get(r, "name")?, description: get(r, "description")? }))
            .collect::<Result<Vec<_>, DbErr>>()?;
        let items = db
            .query_all(
                &Query::select()
                    .columns([PlanItems::Id, PlanItems::LedgerId, PlanItems::Kind, PlanItems::Name, PlanItems::Amount])
                    .from(PlanItems::Table)
                    .order_by(PlanItems::LedgerId, Order::Asc)
                    .order_by(PlanItems::SortOrder, Order::Asc)
                    .order_by(PlanItems::Id, Order::Asc)
                    .to_owned(),
            )
            .await?
            .iter()
            .map(|r| {
                let kind: String = get(r, "kind")?;
                Ok(LegacyItem {
                    id: get(r, "id")?,
                    ledger_id: get(r, "ledger_id")?,
                    kind: match kind.as_str() {
                        "INCOME" => LegacyKind::Income,
                        "ALLOCATION" => LegacyKind::Allocation,
                        _ => LegacyKind::Other,
                    },
                    name: get(r, "name")?,
                    amount: get(r, "amount")?,
                })
            })
            .collect::<Result<Vec<_>, DbErr>>()?;

        for (id, conversion) in legacy_plan::convert(&ledgers, &items) {
            let item = items.iter().find(|i| i.id == id).expect("converted item exists");
            let rule = match conversion {
                Conversion::Keep => continue,
                Conversion::ReplacedByTransfer => None,
                Conversion::Transfer { to } => Some(NewRule {
                    ledger_id: item.ledger_id,
                    description: &item.name,
                    amount: item.amount,
                    category_id: None,
                    transfer_ledger_id: Some(to),
                    day_of_month: legacy_plan::TRANSFER_DAY,
                }),
                Conversion::RegularIncome => Some(NewRule {
                    ledger_id: item.ledger_id,
                    description: &item.name,
                    amount: item.amount,
                    category_id: Some(regular_income_category(db, item.ledger_id).await?),
                    transfer_ledger_id: None,
                    day_of_month: legacy_plan::INCOME_DAY,
                }),
            };
            if let Some(rule) = rule {
                insert_rule(db, rule, start_month, now).await?;
            }
            db.execute(&Query::delete().from_table(PlanItems::Table).and_where(Expr::col(PlanItems::Id).eq(id)).to_owned())
                .await?;
        }
        Ok(())
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            convert(manager.get_connection(), &this_month()).await
        }

        fn use_transaction(&self) -> Option<bool> {
            Some(true)
        }
    }
}

/// 소비 합계 제외 표시는 장부 분리로 바뀌어 쓰지 않는다 (앞 마이그레이션에서 해당 카테고리를 장부로 옮겼다).
mod m20261120_000008_drop_excluded_from_spending {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261120_000008_drop_excluded_from_spending"
        }
    }

    #[derive(DeriveIden)]
    enum Categories {
        Table,
        ExcludedFromSpending,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .alter_table(Table::alter().table(Categories::Table).drop_column(Categories::ExcludedFromSpending).to_owned())
                .await
        }
    }
}

/// 반복 규칙의 자동 입력 매칭 금액 범위 (양끝 포함, 둘 다 비우면 범위 없음).
/// SQLite는 ALTER TABLE 한 번에 컬럼 하나만 더할 수 있어 나눠 추가한다.
mod m20261120_000009_recurring_match_amount {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261120_000009_recurring_match_amount"
        }
    }

    #[derive(DeriveIden)]
    enum RecurringRules {
        Table,
        MatchAmountMin,
        MatchAmountMax,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            for col in [RecurringRules::MatchAmountMin, RecurringRules::MatchAmountMax] {
                manager
                    .alter_table(Table::alter().table(RecurringRules::Table).add_column(big_integer_null(col)).to_owned())
                    .await?;
            }
            Ok(())
        }
    }
}

/// 반복 규칙에 미리 골라 둔 카드 혜택. 예정 거래를 만들 때 거래 혜택으로 옮긴다.
/// 새 테이블이라 외래 키를 함께 만든다 (규칙·혜택을 지우면 같이 지워진다).
mod m20261120_000010_recurring_rule_benefits {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261120_000010_recurring_rule_benefits"
        }
    }

    #[derive(DeriveIden)]
    enum RecurringRuleBenefits {
        Table,
        RecurringRuleId,
        CardBenefitId,
    }

    #[derive(DeriveIden)]
    enum RecurringRules {
        Table,
        Id,
    }

    #[derive(DeriveIden)]
    enum CardBenefits {
        Table,
        Id,
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(RecurringRuleBenefits::Table)
                        .col(integer(RecurringRuleBenefits::RecurringRuleId))
                        .col(integer(RecurringRuleBenefits::CardBenefitId))
                        .primary_key(
                            Index::create()
                                .col(RecurringRuleBenefits::RecurringRuleId)
                                .col(RecurringRuleBenefits::CardBenefitId),
                        )
                        .foreign_key(
                            ForeignKey::create()
                                .name("fk_recurring_rule_benefits_rule")
                                .from(RecurringRuleBenefits::Table, RecurringRuleBenefits::RecurringRuleId)
                                .to(RecurringRules::Table, RecurringRules::Id)
                                .on_delete(ForeignKeyAction::Cascade)
                                .on_update(ForeignKeyAction::Cascade),
                        )
                        .foreign_key(
                            ForeignKey::create()
                                .name("fk_recurring_rule_benefits_card_benefit")
                                .from(RecurringRuleBenefits::Table, RecurringRuleBenefits::CardBenefitId)
                                .to(CardBenefits::Table, CardBenefits::Id)
                                .on_delete(ForeignKeyAction::Cascade)
                                .on_update(ForeignKeyAction::Cascade),
                        )
                        .to_owned(),
                )
                .await?;
            manager
                .create_index(
                    Index::create()
                        .name("ix_recurring_rule_benefits_card_benefit")
                        .table(RecurringRuleBenefits::Table)
                        .col(RecurringRuleBenefits::CardBenefitId)
                        .to_owned(),
                )
                .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ColumnTrait, ConnectOptions, ConnectionTrait, Database, EntityTrait, QueryFilter, QueryOrder};

    use crate::entity::{category, enums::PlanItemKind, ledger, plan_item, recurring_rule, transaction};

    const LEGACY: &str = r#"
INSERT INTO ledgers (id, name, description, is_default, sort_order, created_at) VALUES
  (1, '가계부', '월급사용내역 (생활비)', 1, 0, '2026-10-23T05:17:54.236018+00:00'),
  (2, '용돈', '재홍 용돈', 0, 1, '2026-10-23T05:17:54.236018+00:00');
INSERT INTO categories (id, ledger_id, name, kind, monthly_budget, is_fixed, excluded_from_spending, color, sort_order, archived) VALUES
  (1, 1, '외식', 'EXPENSE', 500000, 0, 0, NULL, 0, 0),
  (9, 1, '경조사비', 'EXPENSE', 250000, 0, 1, NULL, 8, 0),
  (10, 1, '여행', 'EXPENSE', NULL, 0, 1, NULL, 9, 0),
  (11, 1, '기타수익', 'INCOME', NULL, 0, 0, NULL, 10, 0),
  (12, 2, '점심', 'EXPENSE', 90000, 0, 0, NULL, 0, 0);
INSERT INTO plan_items (ledger_id, kind, name, amount, member, sort_order) VALUES
  (1, 'INCOME', '월급', 4500000, NULL, 0),
  (1, 'ALLOCATION', '재홍 용돈', 200000, NULL, 1),
  (1, 'MEMBER_ALLOWANCE', '송화 용돈', 500000, '송화', 2),
  (1, 'ALLOCATION', '경조사비', 250000, NULL, 3),
  (1, 'ALLOCATION', '저축', 100000, NULL, 4),
  (1, 'ALLOCATION', '여행적금', 250000, NULL, 5),
  (2, 'INCOME', '용돈', 200000, NULL, 0);
INSERT INTO transactions (ledger_id, date, amount, category_id, payment_method_id, description, memo, member, status,
  installment_months, exclude_from_performance, recurring_rule_id, recurring_month, source, external_id, raw_text, created_at, updated_at) VALUES
  (1, '2026-02-18', 100000, 9, NULL, '결혼식', NULL, NULL, 'CONFIRMED', 1, 0, NULL, NULL, 'IMPORT', NULL, NULL,
    '2026-10-23T05:17:54.236018+00:00', '2026-10-23T05:17:54.236018+00:00'),
  (1, '2026-07-10', 300000, 10, NULL, '항공권', NULL, NULL, 'CONFIRMED', 1, 0, NULL, NULL, 'IMPORT', NULL, NULL,
    '2026-10-23T05:17:54.236018+00:00', '2026-10-23T05:17:54.236018+00:00'),
  (1, '2026-07-11', 20000, 1, NULL, '점심', NULL, NULL, 'CONFIRMED', 1, 0, NULL, NULL, 'IMPORT', NULL, NULL,
    '2026-10-23T05:17:54.236018+00:00', '2026-10-23T05:17:54.236018+00:00');
"#;

    #[tokio::test]
    async fn converts_legacy_plan_to_ledgers_and_recurring_rules() {
        let mut opts = ConnectOptions::new("sqlite::memory:".to_owned());
        opts.max_connections(1).min_connections(1).sqlx_logging(false);
        let db = Database::connect(opts).await.unwrap();
        // 분할 컬럼까지(5개) 적용한 DB에 예전 방식 데이터를 넣고 나머지 마이그레이션을 돌린다
        Migrator::up(&db, Some(5)).await.unwrap();
        db.execute_unprepared(LEGACY).await.unwrap();
        Migrator::up(&db, None).await.unwrap();

        let ledgers: Vec<(i32, String, i32)> = ledger::Entity::find()
            .order_by_asc(ledger::Column::SortOrder)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|l| (l.id, l.name, l.sort_order))
            .collect();
        assert_eq!(ledgers, vec![(1, "가계부".into(), 0), (2, "용돈".into(), 1), (3, "경조사비".into(), 2), (4, "여행".into(), 3)]);

        // 합계 제외 카테고리는 거래와 함께 같은 이름의 장부로
        let cat = |id: i32| {
            let db = db.clone();
            async move { category::Entity::find_by_id(id).one(&db).await.unwrap().unwrap() }
        };
        let gyeongjo = cat(9).await;
        assert_eq!((gyeongjo.ledger_id, gyeongjo.sort_order, gyeongjo.monthly_budget), (3, 0, Some(250_000)));
        assert_eq!(cat(10).await.ledger_id, 4);
        let txs: Vec<(String, i32, Option<i32>)> = transaction::Entity::find()
            .order_by_asc(transaction::Column::Id)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|t| (t.description, t.ledger_id, t.category_id))
            .collect();
        assert_eq!(txs, vec![("결혼식".into(), 3, Some(9)), ("항공권".into(), 4, Some(10)), ("점심".into(), 1, Some(1))]);

        // 월급 → 정기수익 항목의 매월 10일 수입, 배분 → 매월 1일 장부 이체
        let income = category::Entity::find()
            .filter(category::Column::LedgerId.eq(1))
            .filter(category::Column::Name.eq("정기수익"))
            .one(&db)
            .await
            .unwrap()
            .expect("정기수익 항목");
        assert_eq!(income.kind, crate::entity::enums::CategoryKind::Income);
        let month = m20261120_000007_convert_legacy_plan::this_month();
        let rules: Vec<(i32, String, i64, Option<i32>, Option<i32>, i32, String)> = recurring_rule::Entity::find()
            .order_by_asc(recurring_rule::Column::Id)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.ledger_id, r.description, r.amount, r.category_id, r.transfer_ledger_id, r.day_of_month, r.start_month))
            .collect();
        assert_eq!(
            rules,
            vec![
                (1, "월급".into(), 4_500_000, Some(income.id), None, 10, month.clone()),
                (1, "재홍 용돈".into(), 200_000, None, Some(2), 1, month.clone()),
                (1, "경조사비".into(), 250_000, None, Some(3), 1, month.clone()),
                (1, "여행적금".into(), 250_000, None, Some(4), 1, month.clone()),
            ]
        );
        // 용돈 장부의 고정 수입은 이체가 대신하고, 송화 용돈·저축은 남는다
        let items: Vec<(i32, PlanItemKind, String)> = plan_item::Entity::find()
            .order_by_asc(plan_item::Column::Id)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| (p.ledger_id, p.kind, p.name))
            .collect();
        assert_eq!(items, vec![(1, PlanItemKind::MemberAllowance, "송화 용돈".into()), (1, PlanItemKind::Allocation, "저축".into())]);
        // 합계 제외 컬럼은 없어졌다
        assert!(db.execute_unprepared("SELECT excluded_from_spending FROM categories").await.is_err());
    }
}
