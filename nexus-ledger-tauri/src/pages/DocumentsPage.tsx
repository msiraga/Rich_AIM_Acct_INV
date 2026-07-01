import { useState, useEffect, useCallback, useRef, DragEvent, ChangeEvent } from "react";
import { API_BASE, apiGet } from "../lib/api";
import { useAuth } from "../contexts/AuthContext";

/* ------------------------------------------------------------------ */
/*  Types                                                              */
/* ------------------------------------------------------------------ */

type UploadStatus = "pending" | "uploading" | "success" | "error";

interface FileItem {
  id: string;
  file: File;
  status: UploadStatus;
  progress: number;
  error?: string;
  response?: UploadedDocument;
}

interface UploadedDocument {
  id?: string;
  filename?: string;
  document_type?: string;
  extraction_status?: string;
  extracted_data?: {
    vendor?: string;
    amount?: number;
    date?: string;
    description?: string;
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

interface AiStatus {
  available: boolean;
  ocr_enabled?: boolean;
  models_loaded?: boolean;
  [key: string]: unknown;
}

/* ------------------------------------------------------------------ */
/*  Helpers                                                            */
/* ------------------------------------------------------------------ */

const ACCEPTED_TYPES = [
  "image/png",
  "image/jpeg",
  "image/jpg",
  "image/gif",
  "image/webp",
  "application/pdf",
];

const ACCEPTED_EXTENSIONS = ".png,.jpg,.jpeg,.gif,.webp,.pdf";

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}

function generateId(): string {
  return Math.random().toString(36).substring(2, 10);
}

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

function DocumentsPage() {
  useAuth(); // ensure auth context is available
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [files, setFiles] = useState<FileItem[]>([]);
  const [uploadedDocs, setUploadedDocs] = useState<UploadedDocument[]>([]);
  const [aiStatus, setAiStatus] = useState<AiStatus | null>(null);
  const [aiStatusLoading, setAiStatusLoading] = useState(true);
  const [isDragOver, setIsDragOver] = useState(false);
  const [globalError, setGlobalError] = useState<string | null>(null);

  /* ---- Fetch AI status on mount ---- */
  useEffect(() => {
    apiGet<{ success: boolean; data: AiStatus }>("/api/ai/status")
      .then((res) => {
        if (res.success) {
          setAiStatus(res.data);
        } else {
          setAiStatus({ available: false });
        }
      })
      .catch(() => {
        setAiStatus({ available: false });
      })
      .finally(() => setAiStatusLoading(false));
  }, []);

  /* ---- File validation ---- */
  const isFileAccepted = useCallback((file: File): boolean => {
    return ACCEPTED_TYPES.includes(file.type);
  }, []);

  /* ---- Add files to the queue ---- */
  const addFiles = useCallback(
    (fileList: FileList | File[]) => {
      setGlobalError(null);
      const newItems: FileItem[] = [];
      const rejected: string[] = [];

      Array.from(fileList).forEach((file) => {
        if (!isFileAccepted(file)) {
          rejected.push(file.name);
          return;
        }
        newItems.push({
          id: generateId(),
          file,
          status: "pending",
          progress: 0,
        });
      });

      if (rejected.length > 0) {
        setGlobalError(
          `Rejected files (unsupported type): ${rejected.join(", ")}`
        );
      }

      if (newItems.length > 0) {
        setFiles((prev) => [...prev, ...newItems]);
      }
    },
    [isFileAccepted]
  );

  /* ---- Upload a single file ---- */
  const uploadFile = useCallback(async (item: FileItem): Promise<UploadedDocument | null> => {
    const token = localStorage.getItem("nexus_access_token");

    // Update status to uploading
    setFiles((prev) =>
      prev.map((f) =>
        f.id === item.id ? { ...f, status: "uploading" as UploadStatus, progress: 0 } : f
      )
    );

    const formData = new FormData();
    formData.append("file", item.file);

    try {
      // Use XMLHttpRequest for progress tracking
      return await new Promise<UploadedDocument | null>((resolve, reject) => {
        const xhr = new XMLHttpRequest();

        xhr.upload.addEventListener("progress", (e) => {
          if (e.lengthComputable) {
            const pct = Math.round((e.loaded / e.total) * 100);
            setFiles((prev) =>
              prev.map((f) =>
                f.id === item.id ? { ...f, progress: pct } : f
              )
            );
          }
        });

        xhr.addEventListener("load", () => {
          if (xhr.status >= 200 && xhr.status < 300) {
            try {
              const json = JSON.parse(xhr.responseText);
              const doc: UploadedDocument = json.data || json;
              setFiles((prev) =>
                prev.map((f) =>
                  f.id === item.id
                    ? { ...f, status: "success" as UploadStatus, progress: 100, response: doc }
                    : f
                )
              );
              resolve(doc);
            } catch {
              setFiles((prev) =>
                prev.map((f) =>
                  f.id === item.id
                    ? { ...f, status: "success" as UploadStatus, progress: 100 }
                    : f
                )
              );
              resolve(null);
            }
          } else {
            let errMsg = `Upload failed (${xhr.status})`;
            try {
              const json = JSON.parse(xhr.responseText);
              if (json.error) errMsg = json.error;
            } catch {
              // ignore parse error
            }
            setFiles((prev) =>
              prev.map((f) =>
                f.id === item.id
                  ? { ...f, status: "error" as UploadStatus, error: errMsg }
                  : f
              )
            );
            reject(new Error(errMsg));
          }
        });

        xhr.addEventListener("error", () => {
          const errMsg = "Network error during upload";
          setFiles((prev) =>
            prev.map((f) =>
              f.id === item.id
                ? { ...f, status: "error" as UploadStatus, error: errMsg }
                : f
            )
          );
          reject(new Error(errMsg));
        });

        xhr.open("POST", `${API_BASE}/api/documents/upload`);
        if (token) {
          xhr.setRequestHeader("Authorization", `Bearer ${token}`);
        }
        // Do NOT set Content-Type — browser sets it with multipart boundary
        xhr.send(formData);
      });
    } catch (err) {
      // Error state already set in xhr handlers
      return null;
    }
  }, []);

  /* ---- Upload all pending files ---- */
  const uploadAll = useCallback(async () => {
    setGlobalError(null);
    const pending = files.filter((f) => f.status === "pending");
    if (pending.length === 0) return;

    const docs: UploadedDocument[] = [];
    for (const item of pending) {
      const doc = await uploadFile(item);
      if (doc) docs.push(doc);
    }

    if (docs.length > 0) {
      setUploadedDocs((prev) => [...prev, ...docs]);
    }
  }, [files, uploadFile]);

  /* ---- Remove a file from the queue ---- */
  const removeFile = useCallback((id: string) => {
    setFiles((prev) => prev.filter((f) => f.id !== id));
  }, []);

  /* ---- Clear completed uploads ---- */
  const clearCompleted = useCallback(() => {
    setFiles((prev) => prev.filter((f) => f.status !== "success"));
  }, []);

  /* ---- Drag handlers ---- */
  const handleDragEnter = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setIsDragOver(false);
  }, []);

  const handleDragOver = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback(
    (e: DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.stopPropagation();
      setIsDragOver(false);
      if (e.dataTransfer.files && e.dataTransfer.files.length > 0) {
        addFiles(e.dataTransfer.files);
      }
    },
    [addFiles]
  );

  /* ---- File input change ---- */
  const handleFileSelect = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      if (e.target.files && e.target.files.length > 0) {
        addFiles(e.target.files);
        // Reset so the same file can be re-selected
        e.target.value = "";
      }
    },
    [addFiles]
  );

  /* ---- Derived state ---- */
  const pendingCount = files.filter((f) => f.status === "pending").length;
  const uploadingCount = files.filter((f) => f.status === "uploading").length;
  const errorCount = files.filter((f) => f.status === "error").length;
  const successCount = files.filter((f) => f.status === "success").length;

  return (
    <div className="page">
      <h1>Documents</h1>

      {/* AI Status Badge */}
      <div className="summary-bar">
        <span>
          {files.length} file{files.length !== 1 ? "s" : ""}
          {pendingCount > 0 && ` (${pendingCount} pending)`}
          {uploadingCount > 0 && ` (${uploadingCount} uploading)`}
          {errorCount > 0 && ` (${errorCount} failed)`}
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
          {!aiStatusLoading && aiStatus && (
            <span
              className={`badge ${aiStatus.available ? "badge-success" : "badge-warning"}`}
              title={
                aiStatus.available
                  ? "AI document processing is available"
                  : "AI document processing is not available"
              }
            >
              {aiStatus.available ? "AI Available" : "AI Offline"}
            </span>
          )}
          {aiStatusLoading && (
            <span className="badge">Checking AI...</span>
          )}
        </div>
      </div>

      {globalError && (
        <div className="alert alert-error" style={{ marginBottom: "16px" }}>
          {globalError}
        </div>
      )}

      {/* Drop zone */}
      <div
        className="card"
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onClick={() => fileInputRef.current?.click()}
        style={{
          border: `2px dashed ${isDragOver ? "#4a9eff" : "#ccc"}`,
          borderRadius: "8px",
          padding: "48px 24px",
          textAlign: "center",
          cursor: "pointer",
          backgroundColor: isDragOver ? "rgba(74, 158, 255, 0.05)" : "transparent",
          transition: "border-color 0.2s, background-color 0.2s",
          marginBottom: "24px",
        }}
      >
        <div style={{ fontSize: "48px", marginBottom: "12px", opacity: 0.5 }}>
          {isDragOver ? "\u2B07" : "\uD83D\uDCC4"}
        </div>
        <p style={{ margin: "0 0 8px", fontWeight: 500 }}>
          {isDragOver
            ? "Drop files here"
            : "Drag & drop documents here, or click to browse"}
        </p>
        <p style={{ margin: 0, fontSize: "0.85rem", opacity: 0.6 }}>
          Accepted: PNG, JPEG, GIF, WebP, PDF
        </p>
        <input
          ref={fileInputRef}
          type="file"
          multiple
          accept={ACCEPTED_EXTENSIONS}
          onChange={handleFileSelect}
          style={{ display: "none" }}
        />
      </div>

      {/* Action buttons */}
      {files.length > 0 && (
        <div style={{ display: "flex", gap: "8px", marginBottom: "16px", flexWrap: "wrap" }}>
          {pendingCount > 0 && (
            <button
              className="btn btn-primary"
              onClick={uploadAll}
              disabled={uploadingCount > 0}
            >
              {uploadingCount > 0
                ? `Uploading... (${uploadingCount})`
                : `Upload ${pendingCount} file${pendingCount !== 1 ? "s" : ""}`}
            </button>
          )}
          {successCount > 0 && (
            <button className="btn btn-secondary" onClick={clearCompleted}>
              Clear Completed
            </button>
          )}
        </div>
      )}

      {/* File queue */}
      {files.length > 0 && (
        <div className="card" style={{ marginBottom: "24px" }}>
          <h3 style={{ marginTop: 0 }}>File Queue</h3>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr style={{ textAlign: "left", borderBottom: "1px solid #ddd" }}>
                <th style={{ padding: "8px 12px" }}>Name</th>
                <th style={{ padding: "8px 12px" }}>Size</th>
                <th style={{ padding: "8px 12px" }}>Status</th>
                <th style={{ padding: "8px 12px", width: "80px" }}></th>
              </tr>
            </thead>
            <tbody>
              {files.map((item) => (
                <tr key={item.id} style={{ borderBottom: "1px solid #eee" }}>
                  <td style={{ padding: "8px 12px" }}>
                    {item.file.name}
                  </td>
                  <td style={{ padding: "8px 12px", whiteSpace: "nowrap" }}>
                    {formatBytes(item.file.size)}
                  </td>
                  <td style={{ padding: "8px 12px" }}>
                    {item.status === "pending" && (
                      <span className="badge">Pending</span>
                    )}
                    {item.status === "uploading" && (
                      <div>
                        <span className="badge badge-info">Uploading</span>
                        <div
                          style={{
                            marginTop: "4px",
                            height: "4px",
                            borderRadius: "2px",
                            backgroundColor: "#e0e0e0",
                            overflow: "hidden",
                          }}
                        >
                          <div
                            style={{
                              height: "100%",
                              width: `${item.progress}%`,
                              backgroundColor: "#4a9eff",
                              transition: "width 0.2s",
                            }}
                          />
                        </div>
                      </div>
                    )}
                    {item.status === "success" && (
                      <span className="badge badge-success">Uploaded</span>
                    )}
                    {item.status === "error" && (
                      <span className="badge badge-error" title={item.error}>
                        Error
                      </span>
                    )}
                  </td>
                  <td style={{ padding: "8px 12px", textAlign: "right" }}>
                    {item.status !== "uploading" && (
                      <button
                        className="btn btn-secondary btn-small"
                        onClick={() => removeFile(item.id)}
                        title="Remove"
                      >
                        &times;
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {errorCount > 0 && (
            <div style={{ marginTop: "8px", padding: "8px 12px", fontSize: "0.85rem", color: "#c00" }}>
              {files
                .filter((f) => f.status === "error" && f.error)
                .map((f) => (
                  <div key={f.id}>
                    {f.file.name}: {f.error}
                  </div>
                ))}
            </div>
          )}
        </div>
      )}

      {/* Uploaded documents with extracted data */}
      {uploadedDocs.length > 0 && (
        <div className="card">
          <h3 style={{ marginTop: 0 }}>Uploaded Documents</h3>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr style={{ textAlign: "left", borderBottom: "1px solid #ddd" }}>
                <th style={{ padding: "8px 12px" }}>Document</th>
                <th style={{ padding: "8px 12px" }}>Type</th>
                <th style={{ padding: "8px 12px" }}>Extraction</th>
                <th style={{ padding: "8px 12px" }}>Extracted Data</th>
                <th style={{ padding: "8px 12px" }}></th>
              </tr>
            </thead>
            <tbody>
              {uploadedDocs.map((doc, idx) => (
                <tr key={doc.id || idx} style={{ borderBottom: "1px solid #eee" }}>
                  <td style={{ padding: "8px 12px" }}>
                    {doc.filename || `Document ${idx + 1}`}
                  </td>
                  <td style={{ padding: "8px 12px" }}>
                    {doc.document_type || "—"}
                  </td>
                  <td style={{ padding: "8px 12px" }}>
                    {doc.extraction_status ? (
                      <span
                        className={`badge ${
                          doc.extraction_status === "completed" || doc.extraction_status === "success"
                            ? "badge-success"
                            : doc.extraction_status === "failed"
                            ? "badge-error"
                            : "badge-info"
                        }`}
                      >
                        {doc.extraction_status}
                      </span>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td style={{ padding: "8px 12px", fontSize: "0.85rem" }}>
                    {doc.extracted_data ? (
                      <div>
                        {doc.extracted_data.vendor && (
                          <div><strong>Vendor:</strong> {doc.extracted_data.vendor}</div>
                        )}
                        {doc.extracted_data.amount != null && (
                          <div><strong>Amount:</strong> ${Number(doc.extracted_data.amount).toFixed(2)}</div>
                        )}
                        {doc.extracted_data.date && (
                          <div><strong>Date:</strong> {doc.extracted_data.date}</div>
                        )}
                        {doc.extracted_data.description && (
                          <div><strong>Description:</strong> {doc.extracted_data.description}</div>
                        )}
                      </div>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td style={{ padding: "8px 12px", textAlign: "right" }}>
                    {doc.extracted_data && (
                      <button
                        className="btn btn-primary btn-small"
                        onClick={() => {
                          // Navigate to journal with pre-filled data
                          const params = new URLSearchParams();
                          if (doc.extracted_data?.vendor) params.set("description", doc.extracted_data.vendor);
                          if (doc.extracted_data?.amount) params.set("amount", String(doc.extracted_data.amount));
                          if (doc.extracted_data?.date) params.set("date", doc.extracted_data.date);
                          window.location.href = `/journal?${params.toString()}`;
                        }}
                      >
                        Review Transaction
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Empty state */}
      {files.length === 0 && uploadedDocs.length === 0 && (
        <div className="card" style={{ textAlign: "center", padding: "32px", opacity: 0.6 }}>
          <p>No documents uploaded yet.</p>
          <p style={{ fontSize: "0.85rem" }}>
            Upload receipts, invoices, or other documents for AI-powered data extraction.
          </p>
        </div>
      )}
    </div>
  );
}

export default DocumentsPage;
