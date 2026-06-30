import { useState, useEffect, useRef } from "react";
import { API_BASE } from "../lib/api";

interface ChatMessage {
  type: "system" | "response" | "error";
  intent?: string;
  message: string;
  data?: Record<string, unknown>;
}

function ChatSidebar() {
  const [isOpen, setIsOpen] = useState(false);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [connected, setConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isOpen || wsRef.current) return;

    const accessToken = localStorage.getItem("nexus_access_token");
    if (!accessToken) return;

    // Derive ws:// or wss:// from API_BASE (http:// → ws://, https:// → wss://)
    const wsUrl = API_BASE.replace(/^http/, "ws") + `/ws/chat?token=${encodeURIComponent(accessToken)}`;
    const ws = new WebSocket(wsUrl);

    ws.onopen = () => {
      setConnected(true);
      setMessages((prev) => [
        ...prev,
        { type: "system", message: "Connected to NexusLedger assistant." },
      ]);
    };

    ws.onmessage = (event) => {
      try {
        const msg: ChatMessage = JSON.parse(event.data);
        setMessages((prev) => [...prev, msg]);
      } catch {
        setMessages((prev) => [...prev, { type: "system", message: event.data }]);
      }
    };

    ws.onclose = () => {
      setConnected(false);
      wsRef.current = null;
    };

    ws.onerror = () => {
      setMessages((prev) => [
        ...prev,
        { type: "error", message: "Connection error. Is the server running?" },
      ]);
    };

    wsRef.current = ws;

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, [isOpen]);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const sendMessage = () => {
    if (!input.trim() || !wsRef.current) return;
    wsRef.current.send(input);
    setMessages((prev) => [...prev, { type: "system", message: input, intent: "user" }]);
    setInput("");
  };

  return (
    <>
      <button className="chat-toggle" onClick={() => setIsOpen(!isOpen)} title="Chat with Nexus">
        {isOpen ? "▶" : "💬"}
      </button>

      {isOpen && (
        <aside className="chat-sidebar">
          <div className="chat-header">
            <h3>💬 Nexus Assistant</h3>
            <span className={`chat-status ${connected ? "connected" : "disconnected"}`}>
              {connected ? "● Online" : "○ Offline"}
            </span>
          </div>

          <div className="chat-messages">
            {messages.map((msg, i) => (
              <div key={i} className={`chat-message chat-${msg.type}`}>
                <div className="chat-message-text">{msg.message}</div>
                {msg.data && (
                  <pre className="chat-message-data">{JSON.stringify(msg.data, null, 2)}</pre>
                )}
              </div>
            ))}
            <div ref={messagesEndRef} />
          </div>

          <div className="chat-input-area">
            <input
              type="text"
              className="chat-input"
              placeholder="Ask me anything..."
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && sendMessage()}
            />
            <button className="chat-send-btn" onClick={sendMessage} disabled={!connected}>
              Send
            </button>
          </div>
        </aside>
      )}
    </>
  );
}

export default ChatSidebar;
