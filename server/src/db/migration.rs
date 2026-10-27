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
