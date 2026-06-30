//! Fixed Asset Module
//!
//! Registers fixed assets, computes depreciation schedules, and auto-generates
//! monthly depreciation journal entries.

use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use tracing::info;

use crate::accounting::ledger::Ledger;
use crate::database::financial::{
    Transaction, TransactionEntry, TransactionType, TransactionStatus, EntryType,
};

/// Asset error.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("Asset not found: {0}")]
    NotFound(String),
    #[error("Invalid asset state: {0}")]
    InvalidState(String),
    #[error("Depreciation error: {0}")]
    DepreciationError(String),
    #[error("Ledger error: {0}")]
    LedgerError(String),
    #[error("Account not found in ledger: {0}")]
    AccountNotFound(String),
    #[error("Asset error: {0}")]
    Other(String),
}

pub type AssetResult<T> = Result<T, AssetError>;

/// Depreciation method.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DepreciationMethod {
    /// Straight-line: (cost - salvage) / useful_life_months per month
    StraightLine,
    /// Double declining balance: (2 / useful_life_months) * book_value
    DoubleDeclining,
}

/// A fixed asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedAsset {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub account_id: Uuid,
    pub cost: Decimal,
    pub salvage_value: Decimal,
    pub useful_life_months: u32,
    pub depreciation_method: DepreciationMethod,
    pub acquisition_date: NaiveDate,
    pub disposed_date: Option<NaiveDate>,
    pub accumulated_depreciation: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FixedAsset {
    pub fn new(
        name: &str,
        account_id: Uuid,
        cost: Decimal,
        salvage_value: Decimal,
        useful_life_months: u32,
        acquisition_date: NaiveDate,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: String::new(),
            account_id,
            cost,
            salvage_value,
            useful_life_months,
            depreciation_method: DepreciationMethod::StraightLine,
            acquisition_date,
            disposed_date: None,
            accumulated_depreciation: dec!(0),
            created_at: now,
            updated_at: now,
        }
    }

    /// Compute monthly depreciation expense.
    pub fn monthly_depreciation(&self) -> Decimal {
        match self.depreciation_method {
            DepreciationMethod::StraightLine => {
                if self.useful_life_months == 0 {
                    return dec!(0);
                }
                (self.cost - self.salvage_value) / Decimal::from(self.useful_life_months)
            }
            DepreciationMethod::DoubleDeclining => {
                if self.useful_life_months == 0 {
                    return dec!(0);
                }
                let rate = Decimal::from(2) / Decimal::from(self.useful_life_months);
                let book_value = self.cost - self.accumulated_depreciation;
                let depreciation = book_value * rate;
                // Don't depreciate below salvage value
                if book_value - depreciation < self.salvage_value {
                    book_value - self.salvage_value
                } else {
                    depreciation
                }
            }
        }
    }

    /// Check if the asset is fully depreciated.
    pub fn is_fully_depreciated(&self) -> bool {
        self.accumulated_depreciation >= self.cost - self.salvage_value
    }
}

/// Asset manager — stores assets and generates depreciation entries.
#[derive(Debug, Default)]
pub struct AssetManager {
    pub assets: Vec<FixedAsset>,
}

impl AssetManager {
    pub fn new() -> Self {
        Self { assets: Vec::new() }
    }

    /// Register a new fixed asset.
    pub fn add_asset(&mut self, asset: FixedAsset) -> &FixedAsset {
        self.assets.push(asset);
        self.assets.last().unwrap()
    }

    /// Compute monthly depreciation for all assets and return journal entries.
    ///
    /// Returns tuples of (depreciation_expense_account_id, accumulated_depreciation_account_id, amount).
    /// The caller must post these to the ledger:
    ///   Debit: Depreciation Expense
    ///   Credit: Accumulated Depreciation
    pub fn compute_monthly_depreciation(
        &self,
        as_of: NaiveDate,
        depreciation_expense_account_id: Uuid,
        accumulated_depreciation_account_id: Uuid,
    ) -> Vec<(Uuid, Uuid, Decimal)> {
        let mut entries = Vec::new();

        for asset in &self.assets {
            if asset.is_fully_depreciated() {
                continue;
            }
            if asset.acquisition_date > as_of {
                continue; // not acquired yet
            }
            if let Some(disposed) = asset.disposed_date {
                if disposed < as_of {
                    continue; // already disposed
                }
            }

            let dep = asset.monthly_depreciation();
            if dep > dec!(0) {
                entries.push((depreciation_expense_account_id, accumulated_depreciation_account_id, dep));
            }
        }

        entries
    }

    /// List all assets.
    pub fn list_assets(&self) -> &[FixedAsset] {
        &self.assets
    }

    /// Post monthly depreciation journal entries to the ledger for all
    /// non-disposed, non-fully-depreciated assets as of `as_of_date`.
    ///
    /// For each qualifying asset, creates and posts:
    ///   Dr Depreciation Expense (5050)
    ///   Cr Accumulated Depreciation (1050)
    ///
    /// Updates `accumulated_depreciation` on each asset in place.
    pub async fn post_depreciation(
        &mut self,
        ledger: &Ledger,
        as_of_date: NaiveDate,
    ) -> AssetResult<()> {
        // Resolve the Depreciation Expense (5050) and Accumulated Depreciation (1050)
        // accounts from the ledger's chart of accounts.
        let expense_account_id = ledger
            .get_account_by_number("5050")
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?
            .ok_or_else(|| AssetError::AccountNotFound("5050 (Depreciation Expense)".into()))?
            .id;

        let accum_dep_account_id = ledger
            .get_account_by_number("1050")
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?
            .ok_or_else(|| AssetError::AccountNotFound("1050 (Accumulated Depreciation)".into()))?
            .id;

        // Compute depreciation for all eligible assets
        let dep_entries = self.compute_monthly_depreciation(
            as_of_date,
            expense_account_id,
            accum_dep_account_id,
        );

        if dep_entries.is_empty() {
            info!("No depreciation to post as of {}", as_of_date);
            return Ok(());
        }

        let now = Utc::now();
        let mut total_depreciation = dec!(0);

        // Create a single journal entry for all depreciation
        let mut entries = Vec::new();
        for (_exp_id, _acc_id, amount) in &dep_entries {
            // Debit Depreciation Expense
            entries.push(TransactionEntry {
                id: Uuid::new_v4(),
                account_id: expense_account_id,
                amount: *amount,
                entry_type: EntryType::Debit,
                description: format!("Monthly depreciation — {}", as_of_date.format("%Y-%m")),
                reference: None,
                ..Default::default()
            });
            // Credit Accumulated Depreciation
            entries.push(TransactionEntry {
                id: Uuid::new_v4(),
                account_id: accum_dep_account_id,
                amount: *amount,
                entry_type: EntryType::Credit,
                description: format!("Monthly depreciation — {}", as_of_date.format("%Y-%m")),
                reference: None,
                ..Default::default()
            });
            total_depreciation += *amount;
        }

        let txn = Transaction {
            id: Uuid::new_v4(),
            number: format!("DEP-{}", as_of_date.format("%Y%m")),
            description: format!("Depreciation expense for {}", as_of_date.format("%B %Y")),
            date: as_of_date.and_hms_opt(12, 0, 0).unwrap().and_utc(),
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries,
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({
                "depreciation": true,
                "as_of_date": as_of_date.to_string(),
            }),
            created_at: now,
            updated_at: now,
        };

        ledger
            .record_transaction(txn)
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?;

        // Update accumulated_depreciation on each asset
        for asset in &mut self.assets {
            if asset.is_fully_depreciated() {
                continue;
            }
            if asset.acquisition_date > as_of_date {
                continue;
            }
            if let Some(disposed) = asset.disposed_date {
                if disposed < as_of_date {
                    continue;
                }
            }
            let dep = asset.monthly_depreciation();
            if dep > dec!(0) {
                asset.accumulated_depreciation += dep;
                asset.updated_at = now;
            }
        }

        info!(
            "Posted depreciation of ${} for {} assets as of {}",
            total_depreciation, dep_entries.len(), as_of_date
        );

        Ok(())
    }

    /// Dispose of a fixed asset, recording the sale and computing gain/loss.
    ///
    /// Journal entries posted:
    ///   Dr Cash (sale_price)
    ///   Dr Accumulated Depreciation (1050) — remove accumulated depreciation
    ///   Cr Fixed Assets (1040) — remove original cost
    ///   Cr Gain on Sale (revenue) — if gain (sale_price > book_value)
    ///   Dr Loss on Sale (expense) — if loss (sale_price < book_value)
    ///
    /// Sets `disposed_date` on the asset. Returns the gain (positive) or
    /// loss (negative).
    pub async fn dispose_asset(
        &mut self,
        asset_id: Uuid,
        sale_price: Decimal,
        disposal_date: NaiveDate,
        ledger: &Ledger,
    ) -> AssetResult<Decimal> {
        // Find the asset and clone its data (we need to mutate it later)
        let asset_data = {
            let asset = self
                .assets
                .iter()
                .find(|a| a.id == asset_id)
                .ok_or_else(|| AssetError::NotFound(asset_id.to_string()))?;

            if asset.disposed_date.is_some() {
                return Err(AssetError::InvalidState(format!(
                    "Asset {} is already disposed",
                    asset.name
                )));
            }

            asset.clone()
        };

        // Resolve required accounts from the ledger
        let cash_account_id = ledger
            .get_account_by_number("1000")
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?
            .ok_or_else(|| AssetError::AccountNotFound("1000 (Cash)".into()))?
            .id;

        let accum_dep_account_id = ledger
            .get_account_by_number("1050")
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?
            .ok_or_else(|| AssetError::AccountNotFound("1050 (Accumulated Depreciation)".into()))?
            .id;

        let fixed_asset_account_id = ledger
            .get_account_by_number("1040")
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?
            .ok_or_else(|| AssetError::AccountNotFound("1040 (Fixed Assets)".into()))?
            .id;

        // Calculate book value and gain/loss
        let book_value = asset_data.cost - asset_data.accumulated_depreciation;
        let gain_loss = sale_price - book_value;

        let now = Utc::now();
        let mut entries = Vec::new();

        // Dr Cash (sale_price)
        entries.push(TransactionEntry {
            id: Uuid::new_v4(),
            account_id: cash_account_id,
            amount: sale_price,
            entry_type: EntryType::Debit,
            description: format!("Sale of asset: {}", asset_data.name),
            reference: None,
            ..Default::default()
        });

        // Dr Accumulated Depreciation (remove accumulated depreciation)
        entries.push(TransactionEntry {
            id: Uuid::new_v4(),
            account_id: accum_dep_account_id,
            amount: asset_data.accumulated_depreciation,
            entry_type: EntryType::Debit,
            description: format!("Remove accumulated depreciation: {}", asset_data.name),
            reference: None,
            ..Default::default()
        });

        // Cr Fixed Assets (remove original cost)
        entries.push(TransactionEntry {
            id: Uuid::new_v4(),
            account_id: fixed_asset_account_id,
            amount: asset_data.cost,
            entry_type: EntryType::Credit,
            description: format!("Remove asset cost: {}", asset_data.name),
            reference: None,
            ..Default::default()
        });

        // Post gain or loss
        if gain_loss > dec!(0) {
            // Gain: credit a revenue account
            // Try "Gain on Sale" (4090), fall back to Interest Revenue (4020)
            let gain_account_id = match ledger.get_account_by_number("4090").await {
                Ok(Some(acc)) => acc.id,
                _ => {
                    ledger
                        .get_account_by_number("4020")
                        .await
                        .map_err(|e| AssetError::LedgerError(e.to_string()))?
                        .ok_or_else(|| AssetError::AccountNotFound("Revenue account for gain on sale".into()))?
                        .id
                }
            };

            entries.push(TransactionEntry {
                id: Uuid::new_v4(),
                account_id: gain_account_id,
                amount: gain_loss,
                entry_type: EntryType::Credit,
                description: format!("Gain on sale of asset: {}", asset_data.name),
                reference: None,
                ..Default::default()
            });
        } else if gain_loss < dec!(0) {
            // Loss: debit an expense account
            // Try "Loss on Sale" (5090), fall back to Office Supplies Expense (5040)
            let loss_account_id = match ledger.get_account_by_number("5090").await {
                Ok(Some(acc)) => acc.id,
                _ => {
                    ledger
                        .get_account_by_number("5040")
                        .await
                        .map_err(|e| AssetError::LedgerError(e.to_string()))?
                        .ok_or_else(|| AssetError::AccountNotFound("Expense account for loss on sale".into()))?
                        .id
                }
            };

            let loss_amount = gain_loss.abs(); // positive value for the debit
            entries.push(TransactionEntry {
                id: Uuid::new_v4(),
                account_id: loss_account_id,
                amount: loss_amount,
                entry_type: EntryType::Debit,
                description: format!("Loss on sale of asset: {}", asset_data.name),
                reference: None,
                ..Default::default()
            });
        }

        let txn = Transaction {
            id: Uuid::new_v4(),
            number: format!("DISP-{}", &asset_data.id.to_string()[..8]),
            description: format!("Disposal of asset: {}", asset_data.name),
            date: disposal_date.and_hms_opt(12, 0, 0).unwrap().and_utc(),
            transaction_type: TransactionType::JournalEntry,
            status: TransactionStatus::Pending,
            entries,
            journal_entry_id: None,
            document_ids: vec![],
            metadata: serde_json::json!({
                "asset_disposal": true,
                "asset_id": asset_data.id.to_string(),
                "asset_name": asset_data.name,
                "sale_price": sale_price.to_string(),
                "book_value": book_value.to_string(),
                "gain_loss": gain_loss.to_string(),
            }),
            created_at: now,
            updated_at: now,
        };

        ledger
            .record_transaction(txn)
            .await
            .map_err(|e| AssetError::LedgerError(e.to_string()))?;

        // Set disposed_date on the asset
        let asset = self
            .assets
            .iter_mut()
            .find(|a| a.id == asset_id)
            .unwrap();
        asset.disposed_date = Some(disposal_date);
        asset.updated_at = now;

        info!(
            "Asset {} disposed: sale_price={}, book_value={}, gain/loss={}",
            asset.name, sale_price, book_value, gain_loss
        );

        Ok(gain_loss)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_straight_line_depreciation() {
        // $10,000 asset, $0 salvage, 5 years (60 months) = $166.67/month
        let asset = FixedAsset::new(
            "Laptop",
            Uuid::new_v4(),
            dec!(10000),
            dec!(0),
            60,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        let monthly = asset.monthly_depreciation();
        // $10,000 / 60 = $166.666... 
        assert!((monthly - dec!(166.67)).abs() < dec!(1));
    }

    #[test]
    fn test_straight_line_with_salvage() {
        // $10,000, $1,000 salvage, 5 years → ($9,000 / 60) = $150.00
        let asset = FixedAsset::new(
            "Machine",
            Uuid::new_v4(),
            dec!(10000),
            dec!(1000),
            60,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        assert_eq!(asset.monthly_depreciation(), dec!(150));
    }

    #[test]
    fn test_fully_depreciated() {
        let mut asset = FixedAsset::new(
            "Office Chair",
            Uuid::new_v4(),
            dec!(500),
            dec!(0),
            12,
            NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(),
        );
        asset.accumulated_depreciation = dec!(500);
        assert!(asset.is_fully_depreciated());
    }

    #[test]
    fn test_compute_monthly_depreciation() {
        let mut mgr = AssetManager::new();
        let expense_id = Uuid::new_v4();
        let accum_id = Uuid::new_v4();

        mgr.add_asset(FixedAsset::new(
            "Printer",
            Uuid::new_v4(),
            dec!(1200),
            dec!(0),
            24, // $50/month
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        ));

        let entries = mgr.compute_monthly_depreciation(
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            expense_id,
            accum_id,
        );

        assert_eq!(entries.len(), 1);
        let (exp_id, acc_id, amount) = entries[0];
        assert_eq!(exp_id, expense_id);
        assert_eq!(acc_id, accum_id);
        assert_eq!(amount, dec!(50));
    }

    #[test]
    fn test_asset_manager_add() {
        let mut mgr = AssetManager::new();
        let asset = FixedAsset::new("Test", Uuid::new_v4(), dec!(1000), dec!(0), 36,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        mgr.add_asset(asset);
        assert_eq!(mgr.list_assets().len(), 1);
    }

    #[tokio::test]
    async fn test_post_depreciation() {
        use crate::accounting::ledger::Ledger;

        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        let mut mgr = AssetManager::new();
        mgr.add_asset(FixedAsset::new(
            "Printer",
            Uuid::new_v4(),
            dec!(1200),
            dec!(0),
            24, // $50/month
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        ));

        // Post depreciation for June 2026
        mgr.post_depreciation(&ledger, NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())
            .await
            .unwrap();

        // Verify depreciation expense account (5050) was debited
        let dep_exp = ledger.get_account_by_number("5050").await.unwrap().unwrap();
        assert_eq!(dep_exp.balance, dec!(50));

        // Verify accumulated depreciation account (1050) was credited
        let accum_dep = ledger.get_account_by_number("1050").await.unwrap().unwrap();
        assert_eq!(accum_dep.balance, dec!(-50)); // Contra-asset: credit balance is negative for Asset type

        // Verify asset's accumulated_depreciation was updated
        assert_eq!(mgr.assets[0].accumulated_depreciation, dec!(50));
    }

    #[tokio::test]
    async fn test_dispose_asset_gain() {
        use crate::accounting::ledger::Ledger;

        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        let mut mgr = AssetManager::new();
        let asset = FixedAsset::new(
            "Test Equipment",
            Uuid::new_v4(),
            dec!(10000),
            dec!(0),
            60,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        // Set some accumulated depreciation
        let asset_id = asset.id;
        let mut asset = asset;
        asset.accumulated_depreciation = dec!(2000); // book value = 8000
        mgr.add_asset(asset);

        // Sell for $9000 (gain of $1000)
        let gain_loss = mgr
            .dispose_asset(asset_id, dec!(9000), NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), &ledger)
            .await
            .unwrap();

        assert_eq!(gain_loss, dec!(1000)); // gain
        assert!(mgr.assets[0].disposed_date.is_some());

        // Verify cash increased by 9000
        let cash = ledger.get_account_by_number("1000").await.unwrap().unwrap();
        assert_eq!(cash.balance, dec!(9000));

        // Verify accumulated depreciation was debited (removing the contra-asset balance)
        let accum_dep = ledger.get_account_by_number("1050").await.unwrap().unwrap();
        // Account 1050 starts at 0; the disposal debits it by 2000 (Asset type: debit increases)
        assert_eq!(accum_dep.balance, dec!(2000));

        // Verify fixed assets was credited (reduced)
        let fixed = ledger.get_account_by_number("1040").await.unwrap().unwrap();
        assert_eq!(fixed.balance, dec!(-10000));

        // Verify gain was credited to revenue (4020)
        let gain = ledger.get_account_by_number("4020").await.unwrap().unwrap();
        assert_eq!(gain.balance, dec!(1000));
    }

    #[tokio::test]
    async fn test_dispose_asset_loss() {
        use crate::accounting::ledger::Ledger;

        let mut ledger = Ledger::new();
        ledger.initialize().await.unwrap();

        let mut mgr = AssetManager::new();
        let asset = FixedAsset::new(
            "Old Machine",
            Uuid::new_v4(),
            dec!(5000),
            dec!(0),
            60,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        );
        let asset_id = asset.id;
        let mut asset = asset;
        asset.accumulated_depreciation = dec!(3000); // book value = 2000
        mgr.add_asset(asset);

        // Sell for $1500 (loss of $500)
        let gain_loss = mgr
            .dispose_asset(asset_id, dec!(1500), NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), &ledger)
            .await
            .unwrap();

        assert_eq!(gain_loss, dec!(-500)); // loss
        assert!(mgr.assets[0].disposed_date.is_some());

        // Verify cash increased by 1500
        let cash = ledger.get_account_by_number("1000").await.unwrap().unwrap();
        assert_eq!(cash.balance, dec!(1500));

        // Verify loss was debited to expense (5040)
        let loss = ledger.get_account_by_number("5040").await.unwrap().unwrap();
        assert_eq!(loss.balance, dec!(500));
    }
}
