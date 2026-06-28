use axum::{
    routing::get,
    routing::post,
    Json, Router,
};
use tower_http::cors::{CorsLayer, Any};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct Account {
    id: String,
    name: String,
    account_type: String,
    balance: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Invoice {
    id: String,
    customer: String,
    amount: f64,
    description: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateInvoiceRequest {
    customer: String,
    amount: f64,
    description: String,
}

async fn get_accounts() -> Json<Vec<Account>> {
    let accounts = vec![
        Account {
            id: Uuid::new_v4().to_string(),
            name: "Cash".to_string(),
            account_type: "Asset".to_string(),
            balance: 1000.0,
        },
        Account {
            id: Uuid::new_v4().to_string(),
            name: "Revenue".to_string(),
            account_type: "Revenue".to_string(),
            balance: 5000.0,
        },
    ];
    Json(accounts)
}

async fn get_invoices() -> Json<Vec<Invoice>> {
    let invoices = vec![
        Invoice {
            id: Uuid::new_v4().to_string(),
            customer: "Acme Corp".to_string(),
            amount: 1500.0,
            description: "Consulting Services".to_string(),
            status: "Paid".to_string(),
        },
    ];
    Json(invoices)
}

async fn create_invoice(
    Json(payload): Json<CreateInvoiceRequest>,
) -> Json<Invoice> {
    let invoice = Invoice {
        id: Uuid::new_v4().to_string(),
        customer: payload.customer,
        amount: payload.amount,
        description: payload.description,
        status: "Pending".to_string(),
    };
    Json(invoice)
}

async fn get_ledger() -> Json<Vec<String>> {
    let transactions = vec![
        "Transaction 1: +$1000 (Cash)".to_string(),
        "Transaction 2: +$5000 (Revenue)".to_string(),
    ];
    Json(transactions)
}

async fn reconcile() -> Json<String> {
    Json("Reconciliation completed".to_string())
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(tower_http::cors::Method::GET)
        .allow_methods(tower_http::cors::Method::POST)
        .allow_headers([axum::http::header::CONTENT_TYPE]);

    let app = Router::new()
        .route("/api/accounts", get(get_accounts))
        .route("/api/invoices", get(get_invoices))
        .route("/api/invoices", post(create_invoice))
        .route("/api/ledger", get(get_ledger))
        .route("/api/reconcile", get(reconcile))
        .layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], 4000));
    println!("NexusLedger backend running on http://{}", addr);
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app,
    )
    .await
    .unwrap();
}
