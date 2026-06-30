# Agentic Interaction Design

> **Vision:** NexusLedger is not an accounting tool you operate — it's an agent you talk to. 
> Users interact through natural language (CLI, phone, email, SMS). The system 
> proactively reads emails, downloads attachments, classifies documents, creates 
> transactions, and asks for approval when needed.

---

## The Problem

Every accounting product today requires the user to know accounting:
- "Post a journal entry debiting account 1000 and crediting 4000"
- "Go to Vendors > Bills > New Bill > select vendor > enter line items"
- "Reconcile account 1010 against statement ending balance $12,345"

A business owner doesn't think in debits and credits. They think:
- "I need to bill Acme for last month's consulting"
- "Staples sent me a receipt for $45 — log that"
- "How much cash do I have right now?"
- "Pay the rent"

**NexusLedger must translate human intent into accounting actions — automatically, correctly, and conversationally.**

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                     INTERACTION LAYER                               │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐           │
│  │ CLI Chat  │  │ Phone    │  │ Email    │  │ Web Chat  │          │
│  │ (terminal)│  │ (Twilio) │  │ (IMAP)   │  │(WebSocket)│          │
│  └─────┬─────┘  └─────┬────┘  └─────┬────┘  └─────┬─────┘          │
│        │              │             │              │                 │
│        ▼              ▼             ▼              ▼                 │
│  ┌──────────────────────────────────────────────────────┐          │
│  │           NATURAL LANGUAGE UNDERSTANDING              │          │
│  │  Intent extraction · Entity recognition · Context     │          │
│  │  Disambiguation · Multi-turn conversation             │          │
│  └──────────────────────┬───────────────────────────────┘          │
└─────────────────────────┼───────────────────────────────────────────┘
                          │
                          ▼ (structured Task objects)
┌─────────────────────────────────────────────────────────────────────┐
│                     AGENTIC ACTION LAYER                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐             │
│  │ EmailAgent    │  │ DocIntel     │  │ ApprovalGate │             │
│  │ (IMAP monitor)│  │ Agent        │  │ (high-value) │             │
│  │               │  │ (OCR/PDF)    │  │              │             │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘             │
│         │                 │                 │                      │
│         ▼                 ▼                 ▼                      │
│  ┌──────────────────────────────────────────────────────┐          │
│  │              AGENT ORCHESTRATOR                       │          │
│  │  (existing — 9 agents, event-driven dispatch)        │          │
│  └──────────────────────┬───────────────────────────────┘          │
└─────────────────────────┼───────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     ACCOUNTING ENGINE                               │
│  Ledger · Tax · Payroll · Reconciliation · Audit · SurrealDB       │
│  (existing — Phase 2 complete)                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Layer 1: Interaction Channels

### 1.1 CLI Chat (Terminal)

The primary developer/power-user interface. A REPL that accepts natural language:

```
$ nexusledger chat

> create an invoice for Acme Corp for 10 hours of consulting at $150/hr
✓ Invoice INV-00000001 created for $1,500.00
  Status: Sent (AR entry recorded)
  Customer: Acme Corp
  Due: 2026-07-30

> what's my cash balance?
Your cash balance is $12,345.67 (account 1000).

> pay the Acme invoice
✓ Payment of $1,500.00 processed for invoice INV-00000001
  Dr. Cash (1000) $1,500.00
  Cr. Accounts Receivable (1020) $1,500.00
  Invoice status: Paid

> Staples sent me a receipt for $45.99 for printer paper
✓ Receipt logged
  Vendor: Staples
  Amount: $45.99
  Category: Office Supplies (5040)
  Dr. Office Supplies (5040) $45.99
  Cr. Cash (1000) $45.99
  Would you like me to mark this as approved? (y/n)
```

**Implementation:** Rust binary `nexusledger chat` → opens WebSocket to API server → sends messages → receives responses.

### 1.2 Phone (Twilio)

The most accessible interface — no app, no computer, just call a number.

```
[User calls NexusLedger number]

System: "Hi, this is Nexus. What can I help you with?"

User: "What's my cash balance?"

System: "Your cash balance is twelve thousand three hundred forty-five dollars and sixty-seven cents."

User: "Pay the rent bill"

System: "I found a rent bill for $1,500 to Landlord Corp, due June 30. 
         Should I process the payment?"

User: "Yes"

System: "Payment of $1,500 processed. Your new cash balance is $10,845.67."
```

**Implementation:**
- **Twilio** webhook → API server
- **Speech-to-text**: Whisper (local) or Twilio's built-in STT
- **Text-to-speech**: System generates response text → Twilio TTS
- **Intent mapping**: Same NLU layer as CLI

### 1.3 Email Bot (IMAP)

The system proactively monitors a business email inbox:

```
[Vendor sends invoice email to accounting@company.com]

System detects: 
  - Sender: billing@staples.com → mapped to vendor "Staples"
  - Subject: "Invoice #INV-456 - Office Supplies"
  - Attachment: invoice.pdf (482KB)

System actions:
  1. Download attachment
  2. Extract text from PDF
  3. Parse: Invoice #INV-456, Date 2026-06-15, Total $125.50, Line items: [...]
  4. Classify: Vendor bill → AP entry
  5. Create transaction: Dr. Office Supplies (5040) $125.50, Cr. AP (2000) $125.50
  6. Notify user: "I found a bill from Staples for $125.50. Recorded as Office Supplies."

[Customer sends payment confirmation email]

System detects:
  - Sender: ap@acmecorp.com → mapped to customer "Acme Corp"
  - Subject: "Payment sent - INV-00000001"
  - Body contains: "We have sent payment of $1,500.00 for invoice INV-00000001"

System actions:
  1. Match email to existing invoice
  2. Process payment through InvoiceAgent
  3. Notify user: "Acme Corp paid invoice INV-00000001 ($1,500.00). Marked as Paid."
```

**Implementation:**
- **IMAP client** (rust-imap crate) — polls inbox every 5 minutes
- **Email classification**: LLM-based ("is this a vendor bill, customer payment, bank statement, or other?")
- **Attachment download**: MIME parser, save to temp dir
- **Sender mapping**: Email address → vendor/customer record (with fuzzy matching)

### 1.4 SMS (Twilio)

Quick actions for mobile users:

```
[User texts NexusLedger number]

User: "balance"
System: "Cash: $12,345.67 | AR: $3,200.00 | AP: $850.00 | Net Income YTD: $5,450.00"

User: "create invoice Acme Corp $2000 consulting"
System: "✓ Invoice INV-00000002 created for Acme Corp, $2,000.00. Due 2026-07-30."

User: "reconcile bank"
System: "Starting bank reconciliation for account 1010...
         Found 12 statement transactions. 10 matched automatically. 
         2 need review. Check the app for details."
```

**Implementation:** Same Twilio integration as phone, text-only channel.

### 1.5 Web Chat (Tauri App)

In-app conversational sidebar — always available while viewing financial data:

```
┌────────────────────────────────────────────────────┐
│  NexusLedger                          [Chat ▼]    │
├────────────────────────────┬───────────────────────┤
│                            │  💬 Nexus Assistant    │
│  Balance Sheet             │                       │
│  ┌─────────────────────┐   │  > how much do we     │
│  │ Assets    $45,000   │   │    owe in AP?         │
│  │ Liabilities $12,000 │   │                       │
│  │ Equity    $33,000   │   │  You have 3 unpaid    │
│  └─────────────────────┘   │  bills totaling       │
│                            │  $850.00:             │
│                            │  - Staples $125.50    │
│                            │  - Electric Co $200   │
│                            │  - Landlord $524.50   │
│                            │                       │
│                            │  > pay them all       │
│                            │                       │
│                            │  Processing 3 payments│
│                            │  totaling $850.00...  │
│                            │  ✓ Done. New cash:    │
│                            │    $11,495.67         │
│                            │  [____] [Send]        │
└────────────────────────────┴───────────────────────┘
```

**Implementation:** WebSocket from React frontend → API server → NLU → orchestrator.

---

## Layer 2: Natural Language Understanding (NLU)

### Intent Taxonomy

Every user message is classified into one of these intents:

| Intent | Example | Maps To |
|---|---|---|
| `create_invoice` | "create an invoice for Acme Corp for 10 hours at $150" | `Task::generate_invoice({...})` |
| `process_payment` | "pay the Acme invoice" | `Task::process_payment({...})` |
| `record_receipt` | "log a receipt from Staples for $45.99" | `Task::process_receipt({...})` |
| `record_transaction` | "I received $5000 cash from a sale" | `Task::record_transaction({...})` |
| `calculate_tax` | "how much tax do I owe on $50,000?" | `Task::calculate_taxes({...})` |
| `run_payroll` | "process this week's payroll" | `Task::calculate_payroll({...})` |
| `reconcile` | "reconcile my bank account" | `Task::reconcile_account({...})` |
| `generate_report` | "show me my balance sheet" | `Task::generate_report({...})` |
| `query_balance` | "what's my cash balance?" | Direct ledger query |
| `query_aging` | "who owes me money?" | AR aging report |
| `approve` | "yes, go ahead" | Approval gate |
| `cancel` | "never mind" | Cancel pending action |

### Entity Extraction

From natural language, extract structured entities:

```
"create an invoice for Acme Corp for 10 hours of consulting at $150/hr 
due in 30 days with 7.25% sales tax"

→ {
  intent: "create_invoice",
  customer_name: "Acme Corp",
  items: [{
    description: "consulting",
    quantity: 10,
    unit_price: 150.00
  }],
  due_date: today + 30 days,
  tax_rate: 0.0725
}
```

### Context Memory

Multi-turn conversation with context:

```
> create an invoice for Acme Corp
System: What was the service and amount?
> 10 hours of consulting at $150/hr
System: Got it. Any sales tax?
> yes, California rate
System: ✓ Invoice created for $1,608.75 ($1,500 + $108.75 CA sales tax)
```

### Disambiguation

When intent is ambiguous:

```
> pay Acme's bill
System: I found 2 unpaid invoices for Acme Corp:
  1. INV-00000001 - $1,500.00 (due 2026-06-30)
  2. INV-00000003 - $750.00 (due 2026-07-15)
  Which one?
> the first one
System: ✓ Payment of $1,500.00 processed for INV-00000001
```

### Implementation Approach

**Option A: Local LLM (Ollama)**
- Privacy: all data stays on-device
- Cost: free
- Quality: depends on model (Llama 3, Mistral)
- Latency: 1-5 seconds on local hardware

**Option B: API (OpenAI/Claude)**
- Quality: best available
- Cost: ~$0.01-0.05 per conversation
- Privacy: data sent to cloud
- Latency: 200ms-1s

**Recommendation:** Support both. Default to local (Ollama), fall back to API if local is unavailable or quality is insufficient. The AI module (Phase 5) already has this architecture stubbed.

---

## Layer 3: Agentic Action Layer

### EmailAgent (Proactive)

A new agent type that runs on a timer, not on task submission:

```rust
pub struct EmailAgent {
    config: AgentConfig,
    imap_client: ImapClient,
    sender_mappings: HashMap<String, EntityMapping>, // email → vendor/customer
    document_agent: Arc<DocumentAgent>,
    orchestrator: Arc<AgentOrchestrator>,
}

impl EmailAgent {
    /// Poll inbox every 5 minutes
    pub async fn monitor_inbox(&self) {
        let unread = self.imap_client.fetch_unread().await;
        for email in unread {
            // 1. Classify email type
            let email_type = self.classify_email(&email).await;
            
            // 2. Download attachments
            let attachments = self.download_attachments(&email).await;
            
            // 3. Map sender to entity
            let entity = self.sender_mappings.get(&email.from);
            
            // 4. Create appropriate task
            match email_type {
                EmailType::VendorInvoice => {
                    // Extract invoice data from attachment
                    let invoice_data = self.document_agent.extract_invoice(attachments[0]).await;
                    // Create AP entry
                    let task = Task::process_receipt(invoice_data);
                    self.orchestrator.submit_task(task).await;
                }
                EmailType::CustomerPayment => {
                    // Match to existing invoice
                    let invoice_id = self.match_payment_to_invoice(&email).await;
                    let task = Task::process_payment(json!({invoice_id, amount}));
                    self.orchestrator.submit_task(task).await;
                }
                EmailType::BankStatement => {
                    // Parse statement, create reconciliation task
                    let task = Task::reconcile_account(account_data);
                    self.orchestrator.submit_task(task).await;
                }
                _ => {}
            }
            
            // 5. Notify user
            self.notify_user(&email, email_type).await;
        }
    }
}
```

### DocumentIntelligenceAgent

Processes documents (PDFs, images, receipts) into structured accounting data:

```rust
pub struct DocumentIntelligenceAgent {
    config: AgentConfig,
    ocr_engine: OcrEngine,           // Tesseract
    pdf_extractor: PdfExtractor,      // pdf-extract crate
    ai_service: Arc<AiService>,       // Ollama/API for structured extraction
}

impl DocumentIntelligenceAgent {
    /// Extract structured invoice data from a PDF
    pub async fn extract_invoice(&self, pdf: &[u8]) -> serde_json::Value {
        // 1. Extract text from PDF
        let text = self.pdf_extractor.extract(pdf).await;
        
        // 2. Use AI to parse into structured data
        let prompt = format!(
            "Extract invoice data from this text. Return JSON with fields: 
             invoice_number, invoice_date, due_date, vendor_name, 
             line_items (description, quantity, unit_price, amount),
             subtotal, tax_amount, total.
             Text: {}", text
        );
        
        self.ai_service.generate_text(&prompt).await
    }
    
    /// OCR a receipt image and extract expense data
    pub async fn extract_receipt(&self, image: &[u8]) -> serde_json::Value {
        // 1. OCR the image
        let text = self.ocr_engine.recognize(image).await;
        
        // 2. AI parse
        let prompt = format!(
            "Extract receipt data: vendor_name, date, amount, 
             items, total. Text: {}", text
        );
        
        self.ai_service.generate_text(&prompt).await
    }
}
```

### ApprovalGate

For high-value or uncertain actions, the system asks before recording:

```rust
pub struct ApprovalGate {
    threshold: Decimal,  // Auto-approve below $500
    pending_actions: Vec<PendingAction>,
}

impl ApprovalGate {
    /// Check if an action needs user approval
    pub fn needs_approval(&self, amount: Decimal, action_type: &str) -> bool {
        amount > self.threshold || 
        action_type == "cancel_invoice" ||
        action_type == "delete_transaction"
    }
    
    /// Present pending action to user
    pub async fn request_approval(&self, action: PendingAction) -> bool {
        // In CLI: print prompt and wait for input
        // In phone: speak the prompt and wait for voice response
        // In email: send approval request email
        // In web: show modal
    }
}
```

---

## Layer 4: Voice Integration (Twilio)

### Architecture

```
Phone Call → Twilio → Webhook (API server) → NLU → Orchestrator → Response → TTS → Twilio → Caller
```

### Implementation

```rust
// API endpoint for Twilio webhook
async fn handle_call(req: TwilioRequest) -> TwilioResponse {
    let speech_text = req.speech_result;  // STT output
    
    // Parse intent
    let intent = nlu.parse(&speech_text).await;
    
    // Execute
    let response = execute_intent(intent).await;
    
    // Generate voice response
    TwilioResponse::say(&response.human_readable)
}
```

### Supported Voice Commands

| Voice Command | Action |
|---|---|
| "What's my cash balance?" | Query account 1000 balance |
| "How much do I owe?" | AP aging summary |
| "Who owes me money?" | AR aging summary |
| "Pay the [vendor] bill" | Process payment |
| "Create invoice for [customer] [amount]" | Generate invoice |
| "Log a receipt from [vendor] [amount]" | Process receipt |
| "Run payroll" | Calculate payroll |
| "Show me my profit" | Income statement summary |
| "What's my tax liability?" | Tax calculation |

---

## Implementation Phases

### Phase 3 (Current): API + Frontend + CLI Chat

- [ ] API server with WebSocket support
- [ ] CLI `nexusledger chat` command
- [ ] Basic NLU (intent matching + entity extraction)
- [ ] Tauri React app with chat sidebar
- [ ] No email/phone integration yet

### Phase 4: Auth + Accounting Completeness + Email

- [ ] User authentication (JWT, RBAC)
- [ ] IMAP email monitoring (EmailAgent)
- [ ] Document intelligence (OCR + PDF extraction)
- [ ] Auto-classification of emails and attachments
- [ ] Sender → vendor/customer mapping
- [ ] Auto-bill-entry from vendor emails

### Phase 5: AI Pipeline + Voice

- [ ] Ollama integration for NLU
- [ ] Twilio phone integration
- [ ] SMS command channel
- [ ] Voice command recognition
- [ ] Learning from corrections ("no, that's Office Supplies not Rent")
- [ ] Anomaly detection ("this bill is 3x higher than usual")
- [ ] Proactive insights ("your cash balance is low, you have payroll due Friday")

---

## New Agent Types Required

| Agent | Purpose | Phase |
|---|---|---|
| `ConversationAgent` | Natural language → Task mapping, multi-turn context | P3 |
| `EmailAgent` | IMAP monitoring, email classification, attachment handling | P4 |
| `DocumentIntelligenceAgent` | OCR, PDF extraction, structured data parsing | P5 |
| `VoiceAgent` | Twilio webhook, STT/TTS, voice command processing | P5 |
| `NotificationAgent` | Push notifications, email alerts, SMS alerts | P4 |

---

## User Experience Examples

### Scenario 1: Business Owner Morning Routine

```
[9:00 AM] Owner opens phone, calls NexusLedger

Nexus: "Good morning. You have 3 new emails from vendors, 
        2 unpaid bills totaling $850, and payroll is due Friday."

Owner: "Read me the vendor emails"

Nexus: "First email from Staples, invoice for $125.50, office supplies.
        Second email from Electric Company, bill for $200, utilities.
        Third email from your landlord, rent invoice for $524.50.
        Shall I record all three?"

Owner: "Yes, and schedule the rent payment for Friday with payroll"

Nexus: "Done. I've recorded all three bills and scheduled the rent 
        payment for Friday. Your projected cash balance after Friday 
        will be $9,221.17."
```

### Scenario 2: Quick Invoice via SMS

```
[Owner texts NexusLedger while at a client meeting]

Owner: "invoice ABC Corp 15hrs consulting $175/hr net 15"

Nexus: "✓ Invoice INV-00000005 created
        Customer: ABC Corp
        Amount: $2,625.00
        Due: 2026-07-15
        I'll email it to billing@abccorp.com. OK?"

Owner: "yes"

Nexus: "✓ Invoice emailed. I'll notify you when payment is received."
```

### Scenario 3: Reconciliation by Voice

```
Owner: "Reconcile my checking account"

Nexus: "I found your latest bank statement with ending balance $12,345.67.
        I've matched 15 of 18 transactions automatically.
        3 transactions need your review:

        1. $500 deposit on June 15 — no matching book transaction
        2. $45.99 withdrawal on June 18 — matches Staples receipt
        3. $1,200 transfer on June 20 — matches owner contribution

        Shall I auto-match #2 and #3?"

Owner: "yes, and #1 is a customer payment from XYZ Corp"

Nexus: "Got it. I've created a payment record for XYZ Corp for $500.
        Reconciliation complete. Difference: $0.00."
```

### Scenario 4: Proactive Notification

```
[Push notification at 4:00 PM]

Nexus: "Heads up — your cash balance dropped below $5,000 ($4,827.33).
        You have payroll due Friday ($3,200) and 2 bills due Monday ($850).
        Would you like to transfer funds or delay any payments?"
```

---

## Key Design Principles

1. **Accounting is invisible.** Users never see debits, credits, or account numbers unless they ask. The system handles all double-entry behind the scenes.

2. **Confirmation for irreversible actions.** Payments, invoice cancellations, and large transactions always ask "are you sure?" — unless the user has set up auto-approval rules.

3. **Proactive, not reactive.** The system monitors emails, bank feeds, and due dates. It tells you what needs attention before you ask.

4. **Learn from corrections.** If the user says "no, that's Office Supplies not Rent," the system remembers for next time from that vendor.

5. **Multi-modal.** Same agent intelligence, different channels. A command works the same whether typed, spoken, or emailed.

6. **Privacy-first option.** Local Ollama for NLU means no data leaves the machine. Cloud API is optional.

7. **Graceful degradation.** If Ollama is down, fall back to keyword matching. If email integration isn't configured, the CLI still works. If Twilio isn't set up, SMS is skipped.

---

## What We Have vs What We Need

| Component | Status | Phase |
|---|---|---|
| Accounting engine (9 agents) | ✅ World-class | P2 done |
| API server (axum) | ❌ Stub | P3 |
| CLI chat interface | ❌ None | P3 |
| NLU (intent + entity extraction) | ❌ None | P3 (basic) / P5 (LLM) |
| React frontend with chat | ❌ Skeleton | P3 |
| WebSocket real-time | ❌ None | P3 |
| Email integration (IMAP) | ❌ None | P4 |
| Document intelligence (OCR/PDF) | ❌ None | P5 |
| Twilio phone integration | ❌ None | P5 |
| SMS channel | ❌ None | P5 |
| Proactive monitoring | ❌ None | P4 |
| Learning from corrections | ❌ None | P5 |
| Approval workflow | ❌ None | P3 (basic) / P4 (full) |
