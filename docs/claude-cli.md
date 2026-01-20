# Claude CLI Programmatic Usage

This document covers how to use the `claude` command programmatically with JSON streaming I/O instead of the interactive TUI. This is essential for the agent-sidecar to communicate with Claude Code inside VMs.

## Basic Non-Interactive Mode

Use `--print` (`-p`) to run Claude in non-interactive mode:

```bash
echo "Your prompt" | claude --print
```

## Output Formats

The `--output-format` flag controls how responses are returned. Only works with `--print`.

| Format | Description |
|--------|-------------|
| `text` | Plain text output (default) |
| `json` | Single JSON object with complete result |
| `stream-json` | Newline-delimited JSON events (requires `--verbose`) |

### Text Output (Default)

```bash
echo "Say hello" | claude --print
# Output: Hello! How can I help you today?
```

### JSON Output

```bash
echo "Say hello" | claude --print --output-format json
```

Returns a single JSON object:

```json
{
  "type": "result",
  "subtype": "success",
  "is_error": false,
  "duration_ms": 2203,
  "num_turns": 1,
  "result": "Hello! How can I help you today?",
  "session_id": "142f8494-7219-4ddd-8ee5-5bd3c1a95446",
  "total_cost_usd": 0.052,
  "usage": {
    "input_tokens": 1,
    "output_tokens": 9
  }
}
```

### Stream JSON Output

```bash
echo "Say hello" | claude --print --output-format stream-json --verbose
```

Returns newline-delimited JSON events:

```json
{"type":"system","subtype":"init","session_id":"...","tools":[...],"model":"claude-opus-4-5-20251101"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello!"}]}}
{"type":"result","subtype":"success","result":"Hello!","session_id":"..."}
```

## Input Formats

The `--input-format` flag controls how prompts are sent. Only works with `--print`.

| Format | Description |
|--------|-------------|
| `text` | Plain text prompt from stdin (default) |
| `stream-json` | JSON messages for multi-turn conversations |

### Stream JSON Input Format

Messages must follow this structure:

```json
{"type":"user","message":{"role":"user","content":"Your message here"}}
```

Example:

```bash
echo '{"type":"user","message":{"role":"user","content":"Hello"}}' | \
  claude --print --input-format stream-json --output-format stream-json --verbose
```

## Incremental Token Streaming

Use `--include-partial-messages` with `stream-json` output to receive incremental text deltas:

```bash
echo "Count to 5" | claude --print \
  --output-format stream-json \
  --verbose \
  --include-partial-messages
```

This produces granular streaming events:

```json
{"type":"system","subtype":"init",...}
{"type":"stream_event","event":{"type":"message_start","message":{...}}}
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"1"}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"\n2"}}}
{"type":"stream_event","event":{"type":"content_block_stop","index":0}}
{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}
{"type":"stream_event","event":{"type":"message_stop"}}
{"type":"assistant","message":{...}}
{"type":"result","subtype":"success",...}
```

### Event Types

| Event Type | Description |
|------------|-------------|
| `system` (subtype: `init`) | Session initialization with tools, model, config |
| `stream_event` (message_start) | Start of assistant message |
| `stream_event` (content_block_start) | Start of content block |
| `stream_event` (content_block_delta) | Incremental text chunk |
| `stream_event` (content_block_stop) | End of content block |
| `stream_event` (message_delta) | Message completion info (stop_reason) |
| `stream_event` (message_stop) | End of message |
| `assistant` | Complete assistant message |
| `user` | Tool results (when tools are used) |
| `result` | Final result with usage statistics |

## Multi-Turn Conversations

Send multiple JSON lines to stdin for multi-turn conversations. The session persists within the same process:

```bash
(
  echo '{"type":"user","message":{"role":"user","content":"My name is Alice"}}'
  sleep 2
  echo '{"type":"user","message":{"role":"user","content":"What is my name?"}}'
) | claude --print --input-format stream-json --output-format stream-json --verbose
```

Use `--replay-user-messages` to echo user messages back for acknowledgment:

```bash
claude --print \
  --input-format stream-json \
  --output-format stream-json \
  --verbose \
  --replay-user-messages
```

User messages are echoed with `"isReplay": true`:

```json
{"type":"user","message":{"role":"user","content":"Hello"},"isReplay":true}
```

## Permission Handling

### Bypass Permissions (Sandboxed Environments)

For fully automated execution in sandboxed environments:

```bash
claude --print --dangerously-skip-permissions
```

This sets `"permissionMode": "bypassPermissions"` and auto-approves all tool calls.

### Tool Restrictions

Limit which tools Claude can use:

```bash
# Only allow specific tools
claude --print --allowed-tools "Bash Edit Read"

# Deny specific tools
claude --print --disallowed-tools "Write WebFetch"
```

## Session Management

### Specify Session ID

```bash
claude --print --session-id "550e8400-e29b-41d4-a716-446655440000"
```

### Resume Previous Session

```bash
claude --print --resume "550e8400-e29b-41d4-a716-446655440000"
```

### Continue Most Recent Session

```bash
claude --print --continue
```

### Disable Session Persistence

```bash
claude --print --no-session-persistence
```

## Model Selection

```bash
# Use model alias
claude --print --model sonnet
claude --print --model opus
claude --print --model haiku

# Use full model name
claude --print --model claude-sonnet-4-5-20250929
```

## Budget Limits

```bash
claude --print --max-budget-usd 1.00
```

## Custom System Prompts

```bash
# Replace system prompt
claude --print --system-prompt "You are a helpful coding assistant"

# Append to default system prompt
claude --print --append-system-prompt "Always respond in JSON format"
```

## Full Streaming Example

Complete example for programmatic integration:

```bash
echo '{"type":"user","message":{"role":"user","content":"Write a hello world in Python"}}' | \
  claude --print \
  --input-format stream-json \
  --output-format stream-json \
  --verbose \
  --include-partial-messages \
  --dangerously-skip-permissions \
  --allowed-tools "Bash Edit Read Write" \
  --model sonnet
```

## Tool Execution Events

When Claude uses tools, you'll see tool calls and results in the stream:

```json
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_xxx","name":"Bash","input":{"command":"ls"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_xxx","content":"file1.txt\nfile2.txt"}]}}
```

## Error Handling

Errors are returned with `"is_error": true`:

```json
{
  "type": "result",
  "subtype": "error",
  "is_error": true,
  "error": "Error message here"
}
```

## Reference: Key Flags

| Flag | Description |
|------|-------------|
| `--print`, `-p` | Non-interactive mode (required for programmatic use) |
| `--input-format` | `text` or `stream-json` |
| `--output-format` | `text`, `json`, or `stream-json` |
| `--verbose` | Required for `stream-json` output |
| `--include-partial-messages` | Enable incremental token streaming |
| `--replay-user-messages` | Echo user messages for acknowledgment |
| `--dangerously-skip-permissions` | Auto-approve all tool calls |
| `--allowed-tools` | Whitelist specific tools |
| `--disallowed-tools` | Blacklist specific tools |
| `--model` | Select model (sonnet, opus, haiku, or full name) |
| `--session-id` | Use specific session UUID |
| `--resume` | Resume previous session by ID |
| `--continue`, `-c` | Continue most recent session |
| `--no-session-persistence` | Don't save session to disk |
| `--max-budget-usd` | Limit API spending |
| `--system-prompt` | Custom system prompt |
| `--append-system-prompt` | Append to default system prompt |
