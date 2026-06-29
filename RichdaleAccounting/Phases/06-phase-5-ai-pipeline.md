# Phase 5: AI Pipeline

**Objective:** Build a real AI document processing pipeline. Receipt/invoice images go through OCR → text extraction → AI classification → structured data extraction → auto-created transaction. Anomaly detection and smart categorization.  
**Duration:** 2–3 weeks  
**Depends on:** Phase 4 (freeze token satisfied)  
**Blocks:** Phase 6  

---

## Why This Phase Exists

After Phase 4, the accounting engine is complete but the AI module is entirely stubbed. `extract_text()` returns a placeholder string. The AI prompts say "(binary data, N bytes)" because the document content is never interpreted. Embeddings are generated but never stored or searched. Anomaly detection is designed but never called. This phase makes the "agentic" promise real: the system should autonomously process documents and flag anomalies.

---

## Task List

| ID | Task | Primary File(s) | Depends On | Parallel With |
|---|---|---|---|---|
| 5.1 | **OCR engine integration** — add Tesseract (via `tesseract` crate) or a cloud vision API for extracting text from images. Support PNG, JPEG, and scanned PDFs. | `ai/ocr.rs` (new) | P4 | 5.2 |
| 5.2 | **PDF text extraction** — use `pdf-extract` crate (or similar) to extract text from native PDFs (not scanned). Complement OCR for non-image PDFs. | `ai/pdf.rs` (new) | P4 | 5.1 |
| 5.3 | **Document upload UI** — drag-and-drop zone on frontend. Accept images and PDFs. POST to `/api/documents/upload`. | `nexus-ledger-tauri/src/pages/Documents.tsx` (new) | P4 | 5.1, 5.2 |
| 5.4 | **Wire OCR → AI extraction** — replace the placeholder in `extract_data()`. Flow: document bytes → OCR/PDF extract → text → LLM prompt → structured JSON. | `ai/mod.rs` | 5.1, 5.2 | Nothing |
| 5.5 | **Auto-create transaction from extraction** — when AI extracts vendor + amount + date from a receipt, automatically create a `Transaction` via `LedgerAgent` with correct debit/credit entries. | `agents/document.rs` | 5.4 | Nothing |
| 5.6 | **Embedding storage + vector search** — store document embeddings in SurrealDB. Implement similarity search (cosine distance) to find related documents. | `ai/embeddings.rs` (new), `database/` | P4 | 5.4, 5.5 |
| 5.7 | **Transaction anomaly detection** — on every new transaction, run `analyze_transaction()`. Flag: duplicate amounts within 7 days, unusual vendors, round-number amounts above threshold. Surface alerts in the UI. | `ai/analysis.rs` (new) | P4 | 5.4 |
| 5.8 | **Smart account categorization** — when creating a new account or importing a transaction, suggest the most likely account type using `suggest_account_category()`. Show suggestion in UI with accept/reject. | `ai/classification.rs` (new) | P4 | 5.4 |
| 5.9 | **AI health endpoint** — `GET /api/ai/status` returns: Ollama connected?, model available?, last inference time, error count. | `api/routes/ai.rs` (new) | 5.4 | 5.6, 5.7 |
| 5.10 | **E2E test** — upload a receipt image → OCR extracts text → AI extracts {vendor, amount, date} → transaction auto-created → appears in ledger. | `tests/integration/ai.rs` (new) | 5.5 | Nothing |

---

## Dependency Graph

```
                    P4 (freeze token)
                         │
              ┌──────────┼──────────┐
              │          │          │
         Track A     Track B    Track C
         (OCR)       (UI)       (AI features)
              │          │          │
         5.1 ─┐     5.3       5.6 ─┐
         5.2 ─┘                  5.7 ─┤
              │                    5.8 ─┤
         ┌────┴────┐                 5.9 ─┘
         │   5.4   │  ← Wire OCR → AI
         └────┬────┘
         ┌────┴────┐
         │   5.5   │  ← Auto-create transaction
         └────┬────┘
         ┌────┴────┐
         │  5.10   │  ← E2E test
         └─────────┘
```

---

## Parallel Execution Strategy

**Session 1 (Three parallel tracks):**
- Track A: 5.1 + 5.2 (OCR + PDF extraction)
- Track B: 5.3 (upload UI)
- Track C: 5.6 + 5.7 + 5.8 (embedding, anomaly, categorization — independent)

**Session 2 (After 5.1+5.2, sequential):**
- 5.4 → 5.5 → 5.9 → 5.10

---

## Freeze Token 5 🔒

All conditions must be true:

- [ ] Upload a receipt photo (PNG/JPEG) → OCR extracts readable text
- [ ] Upload a PDF invoice → text extracted from PDF content
- [ ] Extracted text → AI prompts → structured JSON with vendor, amount, date, line items
- [ ] Auto-pipeline: receipt upload → transaction created in SurrealDB with correct debit/credit entries
- [ ] Document embeddings are stored in SurrealDB and searchable by similarity
- [ ] Anomaly detection flags at least one test case (e.g., duplicate transaction within 7 days)
- [ ] Account categorization suggests correct type for test account names (e.g., "Office Rent" → Expense)
- [ ] `GET /api/ai/status` returns Ollama connectivity and model availability
- [ ] Frontend has a document upload page with drag-and-drop
- [ ] E2E test: receipt image → transaction in ledger (no human intervention)
- [ ] AI degrades gracefully: if Ollama is unavailable, upload still works (stores document, skips AI processing, logs warning)
- [ ] `cargo test` passes

---

## Notes for Reviewer

- **Ollama must be running** for AI features to work. The code must handle Ollama being unavailable without crashing.
- Tesseract requires system installation (`tesseract-ocr` package on Linux, installer on Windows/macOS). The build should check for it and warn if missing.
- Real OCR quality depends on image quality. The E2E test should use a clean, high-resolution test image.
- Embedding storage in SurrealDB may require a vector extension or manual cosine-distance calculation. If SurrealDB doesn't support vector search natively, implement brute-force comparison in application code.
- This phase is where the "agentic" branding becomes real — the system acts autonomously on uploaded documents.
