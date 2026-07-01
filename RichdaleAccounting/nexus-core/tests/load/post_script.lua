-- ============================================================================
-- post_script.lua — wrk Lua script for POST transaction load testing.
-- ============================================================================
--
-- Generates a balanced double-entry transaction for each request:
--   Dr  Cash      100   (account_id passed as arg[1])
--   Cr  Revenue   100   (account_id passed as arg[2])
--
-- The body matches the POST /api/v1/transactions schema:
--
--   {
--     "description": "Load test transaction #N",
--     "entries": [
--       { "account_id": "<uuid>", "amount": "100", "entry_type": "debit",
--         "description": "Dr Cash" },
--       { "account_id": "<uuid>", "amount": "100", "entry_type": "credit",
--         "description": "Cr Revenue" }
--     ]
--   }
--
-- Usage from load_post.sh:
--   wrk -t4 -c50 -d60s -s post_script.lua \
--       -H "Authorization: Bearer $TOKEN" \
--       -H "Content-Type: application/json" \
--       http://localhost:4000/api/v1/transactions \
--       -- $CASH_ID $REVENUE_ID
--
-- The Authorization and Content-Type headers are provided via wrk -H flags.
-- wrk.format("POST", nil, nil, body) inherits wrk.headers (which includes
-- the -H flags) and the URL from the command line.

-- ── State ───────────────────────────────────────────────────────────────────

local cash_id     = nil
local revenue_id  = nil
local counter     = 0
local error_count = 0

-- ── init: called once per thread before connections are established ─────────

init = function(args)
    if #args >= 2 then
        cash_id    = args[1]
        revenue_id = args[2]
    else
        -- Fallback: read from environment variables
        cash_id    = os.getenv("CASH_ACCOUNT_ID")
        revenue_id = os.getenv("REVENUE_ACCOUNT_ID")
    end

    if not cash_id or not revenue_id then
        error("post_script.lua: missing account IDs. " ..
              "Pass as wrk args (-- CASH_ID REVENUE_ID) or set env vars.")
    end
end

-- ── request: called for each HTTP request ──────────────────────────────────

request = function()
    counter = counter + 1

    -- Each transaction gets a unique description so the server does not
    -- deduplicate. The double-entry is always balanced: Dr 100 / Cr 100.
    local body = string.format(
        [[{"description":"Load test transaction #%d","entries":[
            {"account_id":"%s","amount":"100","entry_type":"debit","description":"Dr Cash"},
            {"account_id":"%s","amount":"100","entry_type":"credit","description":"Cr Revenue"}
        ]}]],
        counter, cash_id, revenue_id
    )

    -- wrk.format(method, path, headers, body)
    --   method  = "POST"
    --   path    = nil  → uses the URL from the command line
    --   headers = nil  → inherits wrk.headers (includes -H flags)
    --   body    = the JSON string above
    return wrk.format("POST", nil, nil, body)
end

-- ── response: called for each HTTP response ────────────────────────────────

response = function(status, headers, body)
    if status >= 400 then
        error_count = error_count + 1
    end
end

-- ── done: called once at the end of the run ────────────────────────────────

done = function(summary, latency, requests)
    local msg = "\n" ..
        "Lua script stats:\n" ..
        "  Requests sent:     %d\n" ..
        "  HTTP errors (4xx): %d\n"
    io.write(string.format(msg, counter, error_count))
end
