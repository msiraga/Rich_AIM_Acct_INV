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
│  │ CLI Chat  │  │ Telegram  │  │ Email    │  │ Web Chat  │          │
│  │ (terminal)│  │ Bot       │  │ (IMAP)   │  │(WebSocket)│          │
│  └─────┬─────┘  └─────┬────┘  └─────┬────┘  └─────┬─────┘          │
│        │              │             │              │                 │
│        │         ┌────┴────┐        │              │                 │
│        │         │WhatsApp │        │              │                 │
│        │         │Business │        │              │                 │
│        │         └────┬────┘        │              │                 │
│        │              │             │              │                 │
│        ▼              ▼             ▼              ▼                 │
│  ┌──────────────────────────────────────────────────────┐          │
│  │           NATURAL LANGUAGE UNDERSTANDING              │          │
│  │  Intent extraction · Entity recognition · Context     │          │
│  │  Disambiguation · Multi-turn conversation             │          │
│  │  Voice message transcription (Whisper)                 │          │
│  └──────────────────────┬───────────────────────────────┘          │
└─────────────────────────┼───────────────────────────────────────────┘
                          │
                          ▼ (structured Task objects)
┌─────────────────────────────────────────────────────────────────────┐
│                     AGENTIC ACTION LAYER                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐             │
│  │ EmailAgent    │  │ DocIntel     │  │ ApprovalGate │             │
│  │ (IMAP monitor)│  │ Agent        │  │ (inline keys)│             │
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

### 1.2 Messaging Bot (Telegram + WhatsApp)

The primary user-facing channel — no app to install, works on any phone, supports 
**push notifications, voice messages, document sharing, and back-and-forth chat**.

**Why Telegram/WhatsApp over Twilio voice calls:**
- **Push notifications** — system proactively alerts user ("new invoice from Staples")
- **Voice messages** — user sends a voice memo, system transcribes with Whisper and acts
- **Document sharing** — user forwards an invoice PDF or photos a receipt, system processes it
- **Asynchronous** — user responds when convenient, no live call pressure
- **Back-and-forth** — natural conversation flow, multi-turn disambiguation
- **Group chats** — accountant + business owner in same chat
- **Free** (Telegram) or low-cost (WhatsApp Business API ~$0.005/msg)
- **Inline keyboards** — Telegram buttons for "Approve / Reject" instead of typing

#### Telegram Bot Example

```
[User opens Telegram chat with @NexusLedgerBot]

User: what are my sales this week?

Nexus: 📊 Sales this week (Jun 24-30):
       Total: $8,450.00 across 5 invoices
       - INV-00000005: Acme Corp $1,500
       - INV-00000006: XYZ Inc $2,200
       - INV-00000007: Delta Co $1,750
       - INV-00000008: Acme Corp $1,000
       - INV-00000009: Beta LLC $2,000
       
       3 unpaid, 2 paid. Outstanding AR: $4,700.

User: create an invoice for Delta Co for 20 hours of dev work at $175/hr

Nexus: ✅ Invoice INV-00000010 created
       Customer: Delta Co
       Description: 20 hours dev work @ $175/hr
       Subtotal: $3,500.00
       Tax (7.25%): $253.75
       Total: $3,753.75
       Due: Jul 30, 2026
       
       [📎 Download PDF]  [📧 Email to Delta Co]  [✅ Approve]

User: 📧 (taps button)

Nexus: ✅ Invoice emailed to billing@deltaco.com.
       I'll notify you when payment is received.

[Later — proactive notification]

Nexus: 🔔 Payment received!
       Delta Co paid INV-00000010 ($3,753.75)
       Cash balance: $16,199.67
```

#### Voice Message Example

```
[User sends a voice message to Telegram bot]

🎤 (voice memo): "log a receipt from Home Depot for 85 dollars 
    for some tools"

Nexus: 🎵 Transcribed: "log a receipt from Home Depot for 85 dollars 
       for some tools"
       
       ✅ Receipt logged:
       Vendor: Home Depot
       Amount: $85.00
       Category: Office Supplies (5040) — best match
       
       Dr. Office Supplies (5040) $85.00
       Cr. Cash (1000) $85.00
       
       Is "Office Supplies" the right category? 
       [✅ Yes]  [🔧 Change to Tools/Equipment]  [❌ Cancel]

User: (taps 🔧)

Nexus: What category?
       [Office Supplies] [Rent] [Utilities] [COGS] [Other]

User: (types) "other - tools"

Nexus: ✅ Updated. Receipt recorded under "Tools" (5040 - Other).
       I'll remember Home Depot → Tools for next time.
```

#### Proactive Notification Example

```
[Push notification at 9:00 AM]

Nexus: 🔔 Good morning! Here's your daily summary:
       
       💰 Cash: $12,345.67
       📥 3 new vendor emails (2 invoices, 1 statement)
       ⚠️ Rent bill due tomorrow ($1,500)
       📋 Payroll due Friday ($3,200)
       
       Projected cash after Friday: $7,645.67
       
       [📋 Review emails]  [💳 Pay rent now]  [📊 Full report]
```

#### Document Forwarding Example

```
[User forwards an email with invoice PDF attachment to the bot]

User: (forwards email from billing@staples.com with invoice.pdf)

Nexus: 📎 Document received from billing@staples.com
       File: invoice.pdf (482 KB)
       
       Processing... ✅
       
       Extracted:
       - Vendor: Staples
       - Invoice #: INV-456
       - Date: Jun 15, 2026
       - Total: $125.50
       - Line items: Printer paper $45.50, Toner $80.00
       
       Category: Office Supplies (5040)
       
       ✅ Bill recorded:
       Dr. Office Supplies (5040) $125.50
       Cr. Accounts Payable (2000) $125.50
       
       Due: Jul 15, 2026
       [💳 Pay now]  [📅 Schedule payment]  [✅ Done]
```

#### WhatsApp Business Example

```
[Same capabilities, delivered via WhatsApp Business API]

User: income statement for last year

Nexus: 📊 Income Statement — Jan 1 to Dec 31, 2025

       Revenue:
         Sales Revenue:     $145,000.00
         Service Revenue:    $87,500.00
         Interest Revenue:      $320.00
       ─────────────────────────────
       Total Revenue:      $232,820.00

       Expenses:
         COGS:              $52,000.00
         Salaries:          $68,000.00
         Rent:              $18,000.00
         Utilities:          $4,800.00
         Office Supplies:    $3,200.00
       ─────────────────────────────
       Total Expenses:     $146,000.00

       Net Income:          $86,820.00

       [📄 Download PDF]  [📊 Compare to 2024]  [💵 Tax estimate]
```

**Implementation:**

**Telegram:**
- `teloxide` crate (Rust Telegram bot framework)
- Webhook mode (API server receives POST from Telegram)
- Supports: text, voice messages (OGG → Whisper STT), documents (PDF/images), 
  inline keyboards (approval buttons), photos (receipt images → OCR)
- Bot token from @BotFather
- Free — no per-message cost

**WhatsApp Business:**
- Meta Cloud API (direct) or Twilio WhatsApp API
- Webhook mode (same as Telegram)
- Supports: text, voice messages, documents, images
- Template messages for proactive notifications (pre-approved by Meta)
- ~$0.005 per conversation (first 1,000 free/month)

**Shared architecture:**
- Both channels feed into the same NLU layer
- Same agent orchestrator processes all requests
- Same approval gate for high-value actions
- Conversation context persists across channels (Telegram session → same context as CLI)

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

### 1.4 SMS (via Telegram/WhatsApp)

Quick actions for mobile users — same bot, text-only:

```
[User texts NexusLedger bot]

User: "balance"
Nexus: "💰 Cash: $12,345.67 | AR: $3,200.00 | AP: $850.00 | Net Income YTD: $5,450.00"

User: "create invoice Acme Corp $2000 consulting"
Nexus: "✓ Invoice INV-00000002 created for Acme Corp, $2,000.00. Due 2026-07-30."

User: "reconcile bank"
Nexus: "Starting bank reconciliation for account 1010...
         Found 12 statement transactions. 10 matched automatically. 
         2 need review. Check the app for details."
```

**Implementation:** Same Telegram/WhatsApp bot, text-only messages.

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

## Layer 4: Messaging Bot Integration (Telegram + WhatsApp)

### Architecture

```
User Message → Telegram/WhatsApp Webhook → API Server → NLU → Orchestrator → Response → Bot Reply
```

### Supported Input Types

| Input Type | Processing | Example |
|---|---|---|
| Text message | NLU intent extraction | "create invoice for Acme $2000" |
| Voice message | Whisper STT → NLU | 🎤 "log receipt from Staples $45" |
| Document (PDF) | PDF extraction → AI parse | Forwarded vendor invoice |
| Photo | OCR → AI parse | Photo of paper receipt |
| Email forward | Parse forwarded email | Vendor bill email with attachment |
| Inline button tap | Direct action mapping | [✅ Approve] [💳 Pay now] |

### Implementation

```rust
// Telegram webhook handler (teloxide crate)
async fn handle_telegram_update(update: TelegramUpdate) -> Response {
    match update {
        TelegramUpdate::Message(msg) => {
            match msg.kind {
                MessageKind::Text(text) => {
                    // Text → NLU → Task
                    let intent = nlu.parse(&text).await;
                    let response = execute_intent(intent).await;
                    bot.send_message(msg.chat.id, response.text).await;
                    
                    // If approval needed, send inline keyboard
                    if response.needs_approval {
                        bot.send_inline_keyboard(msg.chat.id, response.text, 
                            vec![
                                InlineButton::new("✅ Approve", "approve"),
                                InlineButton::new("❌ Cancel", "cancel"),
                            ]).await;
                    }
                }
                MessageKind::Voice(voice) => {
                    // Download voice file → Whisper STT → NLU
                    let ogg_data = bot.download_file(&voice.file_id).await;
                    let transcript = whisper.transcribe(&ogg_data).await;
                    let intent = nlu.parse(&transcript).await;
                    let response = execute_intent(intent).await;
                    bot.send_message(msg.chat.id, 
                        format!("🎵 Transcribed: \"{}\"\n\n{}", transcript, response.text)).await;
                }
                MessageKind::Document(doc) => {
                    // Download document → DocumentIntelligenceAgent
                    let file_data = bot.download_file(&doc.file_id).await;
                    let extracted = doc_agent.extract_document(&file_data, &doc.file_name).await;
                    let task = match extracted.doc_type {
                        DocType::Invoice => Task::process_receipt(extracted.data),
                        DocType::Receipt => Task::process_receipt(extracted.data),
                        DocType::BankStatement => Task::reconcile_account(extracted.data),
                    };
                    orchestrator.submit_task(task).await;
                    bot.send_message(msg.chat.id, 
                        &format!("📎 Processed {}: {}", doc.file_name, extracted.summary)).await;
                }
                MessageKind::Photo(photo) => {
                    // Download photo → OCR → AI parse
                    let image = bot.download_file(&photo.file_id).await;
                    let ocr_text = ocr_engine.recognize(&image).await;
                    let extracted = doc_agent.parse_receipt_text(&ocr_text).await;
                    // ... same as document flow
                }
                _ => {}
            }
        }
        TelegramUpdate::CallbackQuery(query) => {
            // Inline button tap (approval/rejection)
            match query.data.as_str() {
                "approve" => { approve_pending_action(query.from.id).await; }
                "cancel" => { cancel_pending_action(query.from.id).await; }
                _ => {}
            }
        }
    }
}
```

### Proactive Notifications

The system sends unsolicited messages when events occur:

| Trigger | Notification |
|---|---|
| Vendor email received | "📎 New bill from Staples: $125.50. [Approve] [Review]" |
| Invoice paid by customer | "✅ Acme Corp paid INV-00000001 ($1,500). Cash: $13,845.67" |
| Bill due tomorrow | "⚠️ Rent bill due tomorrow ($1,500). [Pay now] [Schedule]" |
| Low cash balance | "⚠️ Cash dropped below $5,000 ($4,827). Payroll due Friday." |
| Payroll due | "📋 Payroll due Friday. 5 employees, est. gross: $8,200. [Run payroll]" |
| Reconciliation complete | "✅ Bank reconciliation complete. 15/18 matched. 2 need review." |
| Anomaly detected | "🚨 Unusual: $5,000 transfer to unknown account. [Investigate]" |
| Daily summary (9 AM) | "🌅 Cash: $12,345 | AR: $3,200 | AP: $850 | 3 emails pending" |

---

## Implementation Phases

### Phase 3 (Current): API + Frontend + CLI Chat

- [ ] API server with WebSocket support
- [ ] CLI `nexusledger chat` command
- [ ] Basic NLU (intent matching + entity extraction)
- [ ] Tauri React app with chat sidebar
- [ ] No email/messaging integration yet

### Phase 4: Auth + Accounting Completeness + Email + Telegram Bot

- [ ] User authentication (JWT, RBAC)
- [ ] **Telegram bot** — text, voice messages, document/photo sharing, inline buttons
- [ ] IMAP email monitoring (EmailAgent)
- [ ] Document intelligence (OCR + PDF extraction)
- [ ] Auto-classification of emails and attachments
- [ ] Sender → vendor/customer mapping
- [ ] Auto-bill-entry from vendor emails
- [ ] Proactive notifications via Telegram
- [ ] WhatsApp Business API (secondary channel)

### Phase 5: AI Pipeline + Voice Messages

- [ ] Ollama integration for NLU
- [ ] Whisper STT for voice messages (Telegram voice memos)
- [ ] Learning from corrections ("no, that's Office Supplies not Rent")
- [ ] Anomaly detection ("this bill is 3x higher than usual")
- [ ] Proactive insights ("your cash balance is low, you have payroll due Friday")
- [ ] Vendor pattern learning (Home Depot → Tools, Staples → Office Supplies)

---

## New Agent Types Required

| Agent | Purpose | Phase |
|---|---|---|
| `ConversationAgent` | Natural language → Task mapping, multi-turn context | P3 |
| `MessagingBotAgent` | Telegram/WhatsApp webhook, voice message STT, document/photo handling, inline buttons, proactive notifications | P4 |
| `EmailAgent` | IMAP monitoring, email classification, attachment handling | P4 |
| `DocumentIntelligenceAgent` | OCR, PDF extraction, structured data parsing | P5 |
| `NotificationAgent` | Proactive alerts (Telegram, email, in-app), daily summaries, threshold triggers | P4 |

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
| Telegram bot (text/voice/docs/buttons) | ❌ None | P4 |
| WhatsApp Business API | ❌ None | P4 |
| Email integration (IMAP) | ❌ None | P4 |
| Document intelligence (OCR/PDF) | ❌ None | P5 |
| Voice message transcription (Whisper) | ❌ None | P5 |
| Proactive notifications | ❌ None | P4 |
| Learning from corrections | ❌ None | P5 |
| Approval workflow | ❌ None | P3 (basic) / P4 (full) |
