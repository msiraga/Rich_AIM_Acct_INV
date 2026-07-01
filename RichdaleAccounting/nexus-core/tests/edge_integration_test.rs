//! Phase 6 Integration Test: Edge & Sync
//!
//! Tests the full offline → online → sync flow:
//! 1. Create fresh SQLite DB
//! 2. Go offline, create 5 balanced transactions locally
//! 3. Verify all have _dirty=true
//! 4. Go online, trigger sync with MockRemoteSyncSource
//! 5. Verify all 5 pushed to remote
//! 6. Verify _dirty cleared
//! 7. Create transaction remotely, pull, verify appears locally
//! 8. Test conflict: same record modified locally and remotely → logged
//! 9. Test encryption: sensitive fields encrypted at rest (AES-256-GCM)
//! 10. Test compression: large blobs compressed (lz4, measurable reduction)

use nexus_core::edge::local_db::LocalDb;
use nexus_core::edge::store::LocalStore;
use nexus_core::edge::tracking::ChangeTracker;
use nexus_core::edge::sync::{SyncEngine, RemoteSyncSource, MockRemoteSyncSource};
use nexus_core::edge::conflict::{ConflictResolver, ConflictResolutionStrategy};
use nexus_core::edge::encryption::FieldEncryptor;
use nexus_core::edge::compression::BlobCompressor;
use nexus_core::database::financial::*;
use rust_decimal_macros::dec;
use chrono::Utc;
use uuid::Uuid;
use std::sync::Arc;

fn make_db() -> Arc<LocalDb> {
    let db = LocalDb::open_in_memory().expect("Failed to open in-memory SQLite");
    db.run_migrations().expect("Failed to run migrations");
    Arc::new(db)
}

fn make_account(number: &str, name: &str, account_type: AccountType) -> Account {
    Account::new(number, name, account_type)
}

fn make_balanced_transaction(
    number: &str,
    description: &str,
    debit_account_id: Uuid,
    credit_account_id: Uuid,
    amount: rust_decimal::Decimal,
) -> Transaction {
    let entries = vec![
        TransactionEntry::new(debit_account_id, EntryType::Debit, amount, "Debit entry"),
        TransactionEntry::new(credit_account_id, EntryType::Credit, amount, "Credit entry"),
    ];
    let mut txn = Transaction::new(description.to_string(), Utc::now(), entries);
    txn.number = number.to_string();
    txn.status = TransactionStatus::Posted;
    txn
}

// ── Test 1: Offline CRUD works against SQLite ──────────────────────────────

#[tokio::test]
async fn test_offline_crud_works_against_sqlite() {
    let db = make_db();
    let store = LocalStore::new(db.clone());

    // Create accounts offline
    let cash = make_account("1000", "Cash", AccountType::Asset);
    let revenue = make_account("4000", "Revenue", AccountType::Revenue);
    store.save_account(&cash).expect("save cash");
    store.save_account(&revenue).expect("save revenue");

    // Read back
    let retrieved = store.get_account(cash.id).expect("get cash");
    assert_eq!(retrieved.number, "1000");
    assert_eq!(retrieved.name, "Cash");
    assert_eq!(retrieved.account_type, AccountType::Asset);

    // List all
    let accounts = store.list_accounts().expect("list accounts");
    assert_eq!(accounts.len(), 2);
}

// ── Test 2: Creating a transaction offline stores it with _dirty=true ──────

#[tokio::test]
async fn test_offline_transaction_marks_dirty() {
    let db = make_db();
    let store = LocalStore::new(db.clone());

    let cash = make_account("1000", "Cash", AccountType::Asset);
    let revenue = make_account("4000", "Revenue", AccountType::Revenue);
    store.save_account(&cash).expect("save cash");
    store.save_account(&revenue).expect("save revenue");

    let txn = make_balanced_transaction(
        "TRX-001", "Sale", cash.id, revenue.id, dec!(1000),
    );
    store.save_transaction(&txn).expect("save transaction");

    // Verify _dirty = 1
    let row = db.query_one(
        "SELECT _dirty FROM transactions WHERE id = ?1",
        rusqlite::params![txn.id.to_string()],
    ).expect("query dirty flag");
    let dirty: i64 = row.get_by_index(0).expect("get dirty flag");
    assert_eq!(dirty, 1, "Transaction should have _dirty=1");

    // Verify change log entry
    let tracker = ChangeTracker::new(db.clone());
    let changes = tracker.get_pending_changes().expect("get changes");
    assert!(changes.iter().any(|c| c.entity_type == "transaction" && c.entity_id == txn.id.to_string()),
        "Change log should contain the transaction");
}

// ── Test 3: Going online triggers sync — dirty records pushed ───────────────

#[tokio::test]
async fn test_sync_pushes_dirty_records_to_remote() {
    let db = make_db();
    let store = LocalStore::new(db.clone());
    let remote = Arc::new(MockRemoteSyncSource::new());
    let engine = SyncEngine::new(db.clone(), remote.clone());

    // Create accounts and 5 balanced transactions offline
    let cash = make_account("1000", "Cash", AccountType::Asset);
    let revenue = make_account("4000", "Revenue", AccountType::Revenue);
    store.save_account(&cash).expect("save cash");
    store.save_account(&revenue).expect("save revenue");

    for i in 1..=5 {
        let txn = make_balanced_transaction(
            &format!("TRX-{:03}", i), &format!("Sale {}", i),
            cash.id, revenue.id, dec!(100) * rust_decimal::Decimal::from(i),
        );
        store.save_transaction(&txn).expect("save transaction");
    }

    // Verify 5 dirty transactions
    let tracker = ChangeTracker::new(db.clone());
    let dirty = tracker.get_dirty_records().expect("get dirty");
    let txn_dirty = dirty.iter().filter(|d| d.entity_type == "transaction").count();
    assert_eq!(txn_dirty, 5, "Should have 5 dirty transactions");

    // Sync (push)
    let result = engine.push_changes().await.expect("push failed");
    assert!(result.pushed >= 5, "Should have pushed at least 5 transactions (plus accounts), got {}", result.pushed);

    // Verify records appear in remote mock
    let pushed_count = remote.record_count().await;
    assert!(pushed_count >= 5, "Remote should have at least 5 records, got {}", pushed_count);
}

// ── Test 4: Pulling remote changes to local ─────────────────────────────────

#[tokio::test]
async fn test_sync_pulls_remote_changes_to_local() {
    let db = make_db();
    let store = LocalStore::new(db.clone());
    let remote = Arc::new(MockRemoteSyncSource::new());

    // Insert a remote account
    let remote_account_id = Uuid::new_v4();
    let remote_account_data = serde_json::json!({
        "id": remote_account_id.to_string(),
        "number": "3000",
        "name": "Remote Revenue",
        "description": "Created on server",
        "account_type": "Revenue",
        "parent_id": null,
        "status": "Active",
        "balance": "0",
        "currency": "USD",
        "is_bank_account": false,
        "bank_details": null,
        "is_reconciled": false,
        "last_reconciled": null,
        "created_at": Utc::now().to_rfc3339(),
        "updated_at": Utc::now().to_rfc3339(),
    });
    remote.insert_record("account", &remote_account_id.to_string(), remote_account_data, Utc::now()).await;

    // Pull
    let engine = SyncEngine::new(db.clone(), remote);
    let result = engine.pull_changes().await.expect("pull failed");
    assert!(result.pulled >= 1, "Should have pulled at least 1 record");

    // Verify account appears locally
    let local_account = store.get_account(remote_account_id);
    assert!(local_account.is_ok(), "Remote account should exist in local SQLite after pull");
    let acct = local_account.unwrap();
    assert_eq!(acct.number, "3000");
    assert_eq!(acct.name, "Remote Revenue");
}

// ── Test 5: Conflict: same record modified locally and remotely → logged ───

#[tokio::test]
async fn test_conflict_logged_not_lost() {
    let db = make_db();
    let store = LocalStore::new(db.clone());

    // Create an account locally
    let mut account = make_account("2000", "Original Name", AccountType::Asset);
    store.save_account(&account).expect("save");

    // Simulate local modification (set _dirty and _modified_at)
    account.name = "Local Modified".to_string();
    store.save_account(&account).expect("save modified");

    // Create conflict resolver
    let resolver = ConflictResolver::new(db.clone(), ConflictResolutionStrategy::RemoteWins);

    // Simulate a conflict (remote version with different name)
    let conflict = nexus_core::edge::sync::SyncConflict {
        entity_type: "account".to_string(),
        entity_id: account.id.to_string(),
        local_modified_at: Utc::now(),
        remote_modified_at: Utc::now(),
        local_data: serde_json::json!({"name": "Local Modified"}),
        remote_data: serde_json::json!({"name": "Remote Modified"}),
    };

    let resolved = resolver.resolve_conflict(&conflict).expect("resolve");
    assert_eq!(resolved.winner, nexus_core::edge::conflict::ConflictWinner::Remote);

    // Verify conflict is logged
    let logs = resolver.get_conflicts().expect("get conflicts");
    assert!(!logs.is_empty(), "Conflict should be logged in audit trail");

    // Verify both versions are preserved
    let log = &logs[0];
    assert!(log.local_data.is_object(), "Local version should be preserved");
    assert!(log.remote_data.is_object(), "Remote version should be preserved");
}

// ── Test 6: Sensitive fields encrypted at rest (AES-256-GCM) ────────────────

#[tokio::test]
async fn test_sensitive_fields_encrypted_at_rest() {
    let password = "user_secure_password_123";
    let salt = FieldEncryptor::generate_salt();
    let key = FieldEncryptor::derive_key(password, &salt);

    // Simulate encrypting bank account details
    let bank_account_number = "1234567890123456";
    let encrypted = FieldEncryptor::encrypt_field(bank_account_number, &key)
        .expect("encrypt failed");

    // Ciphertext should not contain plaintext
    let encrypted_str = String::from_utf8_lossy(&encrypted);
    assert!(!encrypted_str.contains(bank_account_number),
        "Encrypted data should not contain plaintext bank account number");

    // Ciphertext should be longer than plaintext (nonce + tag overhead)
    assert!(encrypted.len() > bank_account_number.len(),
        "Encrypted data should be longer due to nonce + GCM tag");

    // Decrypt should return original
    let decrypted = FieldEncryptor::decrypt_field(&encrypted, &key)
        .expect("decrypt failed");
    assert_eq!(decrypted, bank_account_number,
        "Decrypted value should match original");

    // Test JSON encryption for complex bank details
    let bank_details = serde_json::json!({
        "bank_name": "Richdale Bank",
        "account_number": "1234567890123456",
        "routing_number": "021000021",
    });
    let enc_json = FieldEncryptor::encrypt_json(&bank_details, &key).expect("encrypt json");
    let dec_json = FieldEncryptor::decrypt_json(&enc_json, &key).expect("decrypt json");
    assert_eq!(dec_json, bank_details, "JSON round-trip should preserve data");

    // Wrong key should fail
    let wrong_key = FieldEncryptor::derive_key("wrong_password", &salt);
    assert!(FieldEncryptor::decrypt_field(&encrypted, &wrong_key).is_err(),
        "Decryption with wrong key should fail");
}

// ── Test 7: Large documents compressed (lz4, measurable reduction) ──────────

#[tokio::test]
async fn test_large_documents_compressed() {
    // Create a large blob (receipt-like content — repetitive binary data)
    let large_data: Vec<u8> = (0..50_000).map(|i| (i % 256) as u8).collect();

    // Should compress (above 256 byte threshold)
    assert!(BlobCompressor::should_compress(&large_data),
        "50KB data should trigger compression");

    let compressed = BlobCompressor::compress_if_beneficial(&large_data)
        .expect("compress failed");

    assert!(compressed.is_compressed, "Data should be compressed");
    assert!(compressed.compressed_size < compressed.original_size,
        "Compressed size ({}) should be less than original ({})",
        compressed.compressed_size, compressed.original_size);

    let ratio = compressed.ratio();
    assert!(ratio > 1.0, "Compression ratio should be > 1.0 (got {})", ratio);

    // Verify round-trip
    let decompressed = compressed.decompress().expect("decompress failed");
    assert_eq!(decompressed, large_data,
        "Decompressed data should match original bit-for-bit");

    // Small data should not compress
    let small_data = vec![1u8, 2, 3];
    let small_result = BlobCompressor::compress_if_beneficial(&small_data)
        .expect("compress small");
    assert!(!small_result.is_compressed, "Small data should not be compressed");

    // Test with JSON text (receipt metadata)
    let json_metadata = serde_json::json!({
        "vendor": "Acme Corp",
        "items": [
            {"name": "Widget", "price": "19.99", "qty": 10},
            {"name": "Gadget", "price": "29.99", "qty": 5},
            {"name": "Gizmo", "price": "9.99", "qty": 20},
        ],
        "total": "549.75",
        "tax": "43.98",
        "date": "2026-07-01",
    });
    let json_bytes = serde_json::to_vec(&json_metadata).unwrap();
    let json_compressed = BlobCompressor::compress_if_beneficial(&json_bytes)
        .expect("compress json");
    if json_compressed.is_compressed {
        let json_decompressed = json_compressed.decompress().expect("decompress json");
        assert_eq!(json_decompressed, json_bytes, "JSON round-trip should match");
    }
}

// ── Test 8: Full sync cycle (push + pull in one call) ──────────────────────

#[tokio::test]
async fn test_full_sync_cycle() {
    let db = make_db();
    let store = LocalStore::new(db.clone());
    let remote = Arc::new(MockRemoteSyncSource::new());

    // Create local data
    let cash = make_account("1000", "Cash", AccountType::Asset);
    let revenue = make_account("4000", "Revenue", AccountType::Revenue);
    store.save_account(&cash).expect("save cash");
    store.save_account(&revenue).expect("save revenue");

    let txn = make_balanced_transaction(
        "TRX-FULL-001", "Full sync test", cash.id, revenue.id, dec!(500),
    );
    store.save_transaction(&txn).expect("save txn");

    // Insert remote data
    let remote_account_id = Uuid::new_v4();
    remote.insert_record("account", &remote_account_id.to_string(),
        serde_json::json!({
            "id": remote_account_id.to_string(),
            "number": "5000",
            "name": "Remote Equipment",
            "description": "",
            "account_type": "Asset",
            "parent_id": null,
            "status": "Active",
            "balance": "0",
            "currency": "USD",
            "is_bank_account": false,
            "bank_details": null,
            "is_reconciled": false,
            "last_reconciled": null,
            "created_at": Utc::now().to_rfc3339(),
            "updated_at": Utc::now().to_rfc3339(),
        }),
        Utc::now(),
    ).await;

    let engine = SyncEngine::new(db.clone(), remote.clone());
    let result = engine.sync().await.expect("sync failed");

    assert!(result.pushed > 0, "Should have pushed local changes");
    assert!(result.pulled > 0, "Should have pulled remote changes");
    assert_eq!(result.errors.len(), 0, "Should have no errors");

    // Verify local data was pushed
    let remote_count = remote.record_count().await;
    assert!(remote_count >= 2, "Remote should have at least 2 records from local");

    // Verify remote data was pulled
    let pulled = store.get_account(remote_account_id);
    assert!(pulled.is_ok(), "Remote account should be in local SQLite after sync");
}

// ── Test 9: Sync is idempotent (pushing twice doesn't duplicate) ────────────

#[tokio::test]
async fn test_sync_is_idempotent() {
    let db = make_db();
    let store = LocalStore::new(db.clone());
    let remote = Arc::new(MockRemoteSyncSource::new());

    let cash = make_account("1000", "Cash", AccountType::Asset);
    store.save_account(&cash).expect("save");

    let engine = SyncEngine::new(db.clone(), remote.clone());

    // First push
    let result1 = engine.push_changes().await.expect("first push");
    let count_after_first = remote.record_count().await;

    // Second push (should be no-op — records already marked synced)
    let result2 = engine.push_changes().await.expect("second push");
    let count_after_second = remote.record_count().await;

    assert_eq!(count_after_first, count_after_second,
        "Remote record count should not change on second push (idempotent)");
    assert_eq!(result2.pushed, 0, "Second push should push 0 records");
}

// ── Test 10: Sync status and pending change count ───────────────────────────

#[tokio::test]
async fn test_sync_status_and_pending_count() {
    let db = make_db();
    let store = LocalStore::new(db.clone());
    let tracker = ChangeTracker::new(db.clone());

    // Initially no pending changes
    let state = tracker.get_sync_state().expect("get state");
    assert_eq!(state.pending_changes, 0, "Should start with 0 pending changes");

    // Create some data
    let cash = make_account("1000", "Cash", AccountType::Asset);
    let revenue = make_account("4000", "Revenue", AccountType::Revenue);
    store.save_account(&cash).expect("save");
    store.save_account(&revenue).expect("save");

    let txn = make_balanced_transaction(
        "TRX-STATUS-001", "Status test", cash.id, revenue.id, dec!(250),
    );
    store.save_transaction(&txn).expect("save txn");

    // Should have pending changes
    let state = tracker.get_sync_state().expect("get state");
    assert!(state.pending_changes >= 3, "Should have at least 3 pending changes (2 accounts + 1 transaction)");

    // After mark_all_synced, should be 0
    tracker.mark_all_synced().expect("mark all synced");
    let state = tracker.get_sync_state().expect("get state after mark");
    assert_eq!(state.pending_changes, 0, "Should have 0 pending changes after mark_all_synced");
}
