//! Load test for NexusLedger API.
//!
//! Spawns 100 concurrent tasks, each making 10 API requests (mix of GET and
//! POST), and measures total time, requests/sec, error count, and p50/p90/p99
//! latency (separately for GET and POST).
//!
//! # Prerequisites
//!
//! The NexusLedger API server must be running on http://localhost:8080 with
//! seed data loaded (at least two accounts for balanced transactions).
//!
//! Set the `JWT_SECRET` environment variable to the same value the server uses.
//!
//! # Running
//!
//! This test is marked `#[ignore]` because it requires a running server.
//!
//! ```sh
//! cargo test --test load_test -- --nocapture --ignored
//! ```
//!
//! To adjust concurrency or requests-per-task, set the `LOAD_CONCURRENT` and
//! `LOAD_REQUESTS_PER_TASK` environment variables:
//!
//! ```sh
//! LOAD_CONCURRENT=200 LOAD_REQUESTS_PER_TASK=20 cargo test --test load_test -- --nocapture --ignored
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::json;
use tokio::sync::mpsc;

/// Base URL of the running NexusLedger API server.
const API_BASE: &str = "http://localhost:8080";

/// Default number of concurrent tasks to spawn.
const DEFAULT_CONCURRENT: usize = 100;

/// Default number of API requests each task makes.
const DEFAULT_REQUESTS_PER_TASK: usize = 10;

/// Maximum latency allowed for GET endpoints (p99).
const GET_P99_THRESHOLD: Duration = Duration::from_millis(500);

/// Maximum latency allowed for POST endpoints (p99).
const POST_P99_THRESHOLD: Duration = Duration::from_millis(1000);

/// Per-request timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

// ── Result types ───────────────────────────────────────────────────────────

/// Latency record for a single request, tagged by method type.
#[derive(Debug, Clone)]
struct LatencyRecord {
    duration: Duration,
    is_post: bool,
    status: u16,
    endpoint: &'static str,
}

/// Aggregated results from one concurrent task.
struct TaskResult {
    latencies: Vec<LatencyRecord>,
    errors: usize,
}

// ── Helper functions ───────────────────────────────────────────────────────

/// Read an env var as usize, falling back to the default.
fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Calculate a percentile from a sorted slice of durations.
///
/// `p` is a value in [0, 100]. Uses nearest-rank interpolation.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (p / 100.0 * (sorted.len() as f64)).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// Format a Duration as milliseconds with one decimal place.
fn fmt_ms(d: Duration) -> String {
    format!("{:.1}ms", d.as_secs_f64() * 1000.0)
}

/// Build an HTTP client with appropriate timeouts.
fn build_client() -> Client {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build reqwest client")
}

/// Authenticate against the running server: register a unique test user, then
/// log in and return the JWT access token.
///
/// If registration fails because the username is already taken (409 / 400),
/// falls back to login.
async fn authenticate(client: &Client) -> String {
    let unique = uuid_suffix();
    let username = format!("loadtest_{}", unique);
    let email = format!("loadtest_{}@nexusledger.test", unique);
    let password = "LoadTest123!".to_string(); // meets policy: 8+ chars, letter + digit

    // Attempt registration
    let register_body = json!({
        "username": username,
        "email": email,
        "password": password,
    });

    let resp = client
        .post(&format!("{}/api/auth/register", API_BASE))
        .json(&register_body)
        .send()
        .await
        .expect("register request failed");

    // If registration succeeded (201), extract the token from the response.
    if resp.status().as_u16() == 201 {
        let body: serde_json::Value = resp
            .json()
            .await
            .expect("failed to parse register response");
        return extract_token(&body, "register");
    }

    // If the user already exists (400 "username already taken" or 409), try login.
    let login_body = json!({
        "username": username,
        "password": password,
    });

    let resp = client
        .post(&format!("{}/api/auth/login", API_BASE))
        .json(&login_body)
        .send()
        .await
        .expect("login request failed");

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        panic!(
            "authentication failed: register returned non-201, login returned {} — {}",
            status, text
        );
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .expect("failed to parse login response");
    extract_token(&body, "login")
}

/// Extract the `access_token` field from an auth response body.
fn extract_token(body: &serde_json::Value, context: &str) -> String {
    body.get("data")
        .and_then(|d| d.get("access_token"))
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| {
            panic!(
                "missing access_token in {} response: {}",
                context, body
            )
        })
        .to_string()
}

/// Generate a short unique suffix from the current system time + process ID.
fn uuid_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    format!("{:x}{:x}", nanos, pid)
}

/// Fetch the list of accounts and return the first two account IDs (for
/// balanced debit/credit transactions). Returns an empty Vec if none found.
async fn fetch_account_ids(client: &Client, token: &str) -> Vec<String> {
    let resp = client
        .get(&format!("{}/api/v1/accounts", API_BASE))
        .bearer_auth(token)
        .send()
        .await
        .expect("failed to fetch accounts");

    if !resp.status().is_success() {
        eprintln!("WARNING: GET /api/v1/accounts returned {} — POST transactions will be skipped", resp.status());
        return Vec::new();
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .expect("failed to parse accounts response");

    let accounts = body
        .get("data")
        .and_then(|d| d.as_array())
        .unwrap_or(&Vec::new())
        .clone();

    accounts
        .iter()
        .filter_map(|acc| acc.get("id").and_then(|id| id.as_str()).map(String::from))
        .take(2)
        .collect()
}

/// Build the JSON body for a POST /api/v1/transactions request.
fn build_transaction_body(account_ids: &[String], task_id: usize, req_id: usize) -> serde_json::Value {
    if account_ids.len() < 2 {
        return json!({});
    }
    json!({
        "description": format!("Load test txn — task {} req {}", task_id, req_id),
        "entries": [
            {
                "account_id": account_ids[0],
                "amount": "10.00",
                "entry_type": "debit",
                "description": "Load test debit"
            },
            {
                "account_id": account_ids[1],
                "amount": "10.00",
                "entry_type": "credit",
                "description": "Load test credit"
            }
        ]
    })
}

/// Execute a single API request and return its latency record.
///
/// `endpoint` is one of: "health", "status", "accounts", "transactions",
/// "agents", "tasks_queue", "create_transaction".
async fn make_request(
    client: &Client,
    token: &str,
    endpoint: &'static str,
    account_ids: &[String],
    task_id: usize,
    req_id: usize,
) -> Result<LatencyRecord, String> {
    let url = format!("{}{}", API_BASE, endpoint_path(endpoint));
    let start = Instant::now();

    let resp = match endpoint {
        "health" => client.get(&url).send().await,
        "status" | "accounts" | "transactions" | "agents" | "tasks_queue" => {
            client.get(&url).bearer_auth(token).send().await
        }
        "create_transaction" => {
            let body = build_transaction_body(account_ids, task_id, req_id);
            if body.as_object().map_or(true, |m| m.is_empty()) {
                // No account IDs available — fall back to a GET.
                let r = client
                    .get(&format!("{}/api/v1/transactions", API_BASE))
                    .bearer_auth(token)
                    .send()
                    .await;
                return r
                    .map(|resp| LatencyRecord {
                        duration: start.elapsed(),
                        is_post: false,
                        status: resp.status().as_u16(),
                        endpoint: "transactions",
                    })
                    .map_err(|e| e.to_string());
            }
            client
                .post(&url)
                .bearer_auth(token)
                .json(&body)
                .send()
                .await
        }
        _ => unreachable!("unknown endpoint: {}", endpoint),
    };

    let duration = start.elapsed();

    match resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            // Drain the body so the connection can be reused.
            let _ = resp.bytes().await;
            Ok(LatencyRecord {
                duration,
                is_post: endpoint == "create_transaction",
                status,
                endpoint,
            })
        }
        Err(e) => Err(format!("{}: {}", endpoint, e)),
    }
}

/// Map an endpoint name to its URL path.
fn endpoint_path(endpoint: &str) -> &'static str {
    match endpoint {
        "health" => "/health",
        "status" => "/api/v1/status",
        "accounts" => "/api/v1/accounts",
        "transactions" => "/api/v1/transactions",
        "agents" => "/api/v1/agents",
        "tasks_queue" => "/api/v1/tasks/queue",
        "create_transaction" => "/api/v1/transactions",
        _ => "",
    }
}

/// The sequence of 10 requests each task makes (mix of GET and POST).
const REQUEST_SEQUENCE: &'static [&'static str] = &[
    "health",
    "status",
    "accounts",
    "transactions",
    "create_transaction",
    "transactions",
    "agents",
    "tasks_queue",
    "create_transaction",
    "status",
];

// ── Main load test ─────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires a running NexusLedger API server on localhost:8080"]
async fn load_test_100_concurrent() {
    let concurrent = env_usize("LOAD_CONCURRENT", DEFAULT_CONCURRENT);
    let requests_per_task = env_usize("LOAD_REQUESTS_PER_TASK", DEFAULT_REQUESTS_PER_TASK);

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("  NexusLedger Load Test");
    println!("  Concurrent tasks : {}", concurrent);
    println!("  Requests per task: {}", requests_per_task);
    println!("  Total requests   : {}", concurrent * requests_per_task);
    println!("  API base URL     : {}", API_BASE);
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    // ── Phase 1: Authenticate ───────────────────────────────────────────

    println!("[1/3] Authenticating test user...");
    let client = build_client();
    let token = authenticate(&client).await;
    println!("      ✓ JWT token acquired");

    // ── Phase 2: Fetch account IDs for POST requests ────────────────────

    println!("[2/3] Fetching account IDs for transaction creation...");
    let account_ids = fetch_account_ids(&client, &token).await;
    if account_ids.len() >= 2 {
        println!("      ✓ Got {} account IDs", account_ids.len());
    } else {
        println!("      ⚠ No account IDs found — POST requests will fall back to GET");
    }

    // ── Phase 3: Spawn concurrent tasks ─────────────────────────────────

    println!("[3/3] Spawning {} concurrent tasks ({} requests each)...", concurrent, requests_per_task);
    println!();

    let (tx, mut rx) = mpsc::channel(concurrent);

    let overall_start = Instant::now();

    for task_id in 0..concurrent {
        let tx = tx.clone();
        let token = token.clone();
        let client = client.clone();
        let account_ids = account_ids.clone();

        tokio::spawn(async move {
            let mut latencies = Vec::with_capacity(requests_per_task);
            let mut errors = 0;

            for req_id in 0..requests_per_task {
                // Cycle through the request sequence.
                let endpoint = REQUEST_SEQUENCE[req_id % REQUEST_SEQUENCE.len()];

                match make_request(&client, &token, endpoint, &account_ids, task_id, req_id).await {
                    Ok(record) => {
                        if record.status >= 400 {
                            errors += 1;
                        }
                        latencies.push(record);
                    }
                    Err(e) => {
                        eprintln!("  [task {} req {}] ERROR: {}", task_id, req_id, e);
                        errors += 1;
                    }
                }
            }

            // Send results back; if the receiver is gone, just drop.
            let _ = tx.send(TaskResult { latencies, errors }).await;
        });
    }

    // Drop the original sender so the channel closes after all tasks finish.
    drop(tx);

    // ── Collect results ─────────────────────────────────────────────────

    let mut all_latencies: Vec<LatencyRecord> = Vec::with_capacity(concurrent * requests_per_task);
    let mut total_errors = 0usize;
    let mut tasks_completed = 0usize;

    while let Some(result) = rx.recv().await {
        tasks_completed += 1;
        total_errors += result.errors;
        all_latencies.extend(result.latencies);
    }

    let total_duration = overall_start.elapsed();
    let total_requests = all_latencies.len();
    let requests_per_sec = if total_duration.as_secs_f64() > 0.0 {
        total_requests as f64 / total_duration.as_secs_f64()
    } else {
        0.0
    };

    // ── Separate GET and POST latencies ────────────────────────────────

    let mut get_latencies: Vec<Duration> = all_latencies
        .iter()
        .filter(|r| !r.is_post)
        .map(|r| r.duration)
        .collect();
    let mut post_latencies: Vec<Duration> = all_latencies
        .iter()
        .filter(|r| r.is_post)
        .map(|r| r.duration)
        .collect();

    get_latencies.sort();
    post_latencies.sort();

    let get_p50 = percentile(&get_latencies, 50.0);
    let get_p90 = percentile(&get_latencies, 90.0);
    let get_p99 = percentile(&get_latencies, 99.0);
    let get_min = get_latencies.first().copied().unwrap_or(Duration::ZERO);
    let get_max = get_latencies.last().copied().unwrap_or(Duration::ZERO);

    let post_p50 = percentile(&post_latencies, 50.0);
    let post_p90 = percentile(&post_latencies, 90.0);
    let post_p99 = percentile(&post_latencies, 99.0);
    let post_min = post_latencies.first().copied().unwrap_or(Duration::ZERO);
    let post_max = post_latencies.last().copied().unwrap_or(Duration::ZERO);

    // ── Per-endpoint breakdown ─────────────────────────────────────────

    let mut endpoint_stats: HashMap<&str, (usize, usize)> = HashMap::new();
    for rec in &all_latencies {
        let entry = endpoint_stats.entry(rec.endpoint).or_insert((0, 0));
        entry.0 += 1; // count
        if rec.status >= 400 {
            entry.1 += 1; // errors
        }
    }

    // ── Print results ──────────────────────────────────────────────────

    println!();
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    LOAD TEST RESULTS                          ║");
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  Tasks completed : {:>6}                                      ║", tasks_completed);
    println!("║  Total requests  : {:>6}                                      ║", total_requests);
    println!("║  Total errors    : {:>6}                                      ║", total_errors);
    println!("║  Total duration  : {:>8.2}s                                  ║", total_duration.as_secs_f64());
    println!("║  Requests/sec    : {:>8.1}                                   ║", requests_per_sec);
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  GET  requests   : {:>6}                                      ║", get_latencies.len());
    println!("║  POST requests   : {:>6}                                      ║", post_latencies.len());
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  GET  latency    p50={:<8} p90={:<8} p99={:<8}    ║", fmt_ms(get_p50), fmt_ms(get_p90), fmt_ms(get_p99));
    println!("║  GET  range      min={:<8} max={:<8}                ║", fmt_ms(get_min), fmt_ms(get_max));
    println!("║  POST latency    p50={:<8} p90={:<8} p99={:<8}    ║", fmt_ms(post_p50), fmt_ms(post_p90), fmt_ms(post_p99));
    println!("║  POST range      min={:<8} max={:<8}                ║", fmt_ms(post_min), fmt_ms(post_max));
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  Per-endpoint breakdown:                                      ║");
    let mut endpoints_sorted: Vec<(&str, (usize, usize))> = endpoint_stats.into_iter().collect();
    endpoints_sorted.sort_by(|a, b| a.0.cmp(b.0));
    for (name, (count, errs)) in &endpoints_sorted {
        let display_name = format!("/{}", name.replace('_', "/"));
        println!("║    {:<22} {:>5} reqs, {:>3} errors           ║", display_name, count, errs);
    }
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();

    // ── Assertions ─────────────────────────────────────────────────────

    // 1. All tasks must complete
    assert_eq!(
        tasks_completed, concurrent,
        "Expected {} tasks to complete, got {}",
        concurrent, tasks_completed
    );

    // 2. No errors allowed
    assert_eq!(
        total_errors, 0,
        "No errors allowed — got {} errors out of {} requests",
        total_errors, total_requests
    );

    // 3. GET p99 must be < 500ms
    if !get_latencies.is_empty() {
        assert!(
            get_p99 < GET_P99_THRESHOLD,
            "GET p99 latency ({}) must be < {} — FAIL",
            fmt_ms(get_p99),
            fmt_ms(GET_P99_THRESHOLD)
        );
        println!("✓ GET  p99 ({}) < {} — PASS", fmt_ms(get_p99), fmt_ms(GET_P99_THRESHOLD));
    }

    // 4. POST p99 must be < 1000ms
    if !post_latencies.is_empty() {
        assert!(
            post_p99 < POST_P99_THRESHOLD,
            "POST p99 latency ({}) must be < {} — FAIL",
            fmt_ms(post_p99),
            fmt_ms(POST_P99_THRESHOLD)
        );
        println!("✓ POST p99 ({}) < {} — PASS", fmt_ms(post_p99), fmt_ms(POST_P99_THRESHOLD));
    }

    println!();
    println!("✓ ALL LOAD TEST ASSERTIONS PASSED");
    println!();
}
