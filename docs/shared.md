# Shared Package

The `@lia/shared` package contains TypeScript type definitions and Zod validation schemas shared across the Lia monorepo. It serves as the single source of truth for data contracts between components.

## Architecture

```
packages/shared/
├── src/
│   └── index.ts     # All schemas and types
├── dist/
│   ├── index.js     # Compiled JavaScript
│   └── index.d.ts   # TypeScript declarations
├── package.json
└── tsconfig.json
```

## Task Status

```typescript
export const TaskStatus = {
  Pending: "pending",      // Task created, waiting to start
  Starting: "starting",    // VM is being created
  Running: "running",      // VM running, agent active
  Suspended: "suspended",  // VM paused, storage preserved
  Terminated: "terminated" // Task complete or deleted
} as const;
```

State machine: `pending → starting → running → suspended/terminated`

## Schemas

### TaskConfigSchema

VM resource allocation parameters:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `timeout_minutes` | number | 30 | Task execution timeout |
| `max_memory_mb` | number | 2048 | RAM allocation |
| `vcpu_count` | number | 2 | CPU cores |
| `storage_gb` | number | 50 | Disk space |

### CreateTaskRequestSchema

Task creation payload:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `prompt` | string | Yes | User request (1-100k chars) |
| `user_id` | string | Yes | Discord user ID |
| `guild_id` | string | No | Discord server ID |
| `config` | TaskConfig | No | VM resource overrides |
| `files` | Array<{name, content}> | No | Initial files for VM |
| `ssh_public_key` | string | No | SSH access key |

### TaskResponseSchema

Complete task data model:

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID | Unique identifier |
| `user_id` | string | Discord user ID |
| `guild_id` | string | null | Discord server ID |
| `prompt` | string | User's request |
| `status` | TaskStatus | Current state |
| `vm_id` | string | null | Firecracker VM ID |
| `config` | TaskConfig | null | Resource configuration |
| `created_at` | datetime | Creation timestamp |
| `started_at` | datetime | null | When VM started |
| `completed_at` | datetime | null | When task finished |
| `exit_code` | number | null | Process exit code |
| `error_message` | string | null | Error details |
| `web_url` | string | Frontend URL |
| `ssh_command` | string | null | SSH connection command |
| `ip_address` | string | null | VM IP address |

### TaskListResponseSchema

Paginated task list:

| Field | Type | Description |
|-------|------|-------------|
| `tasks` | TaskResponse[] | Task array |
| `total` | number | Total count |
| `page` | number | Current page |
| `per_page` | number | Items per page |

### ApiErrorSchema

Standard error response:

| Field | Type | Description |
|-------|------|-------------|
| `error` | string | Human-readable message |
| `code` | string | Optional error code |
| `details` | object | Optional context |

## WebSocket Messages

### Message Types

```typescript
export const WsMessageType = {
  Output: "output",    // Terminal output
  Input: "input",      // User input
  Status: "status",    // Task status change
  Error: "error",      // Error notification
  Ping: "ping",        // Keep-alive request
  Pong: "pong"         // Keep-alive response
} as const;
```

### Message Schemas

**WsOutputMessage** - Terminal output streaming:
```typescript
{ type: "output", data: string, timestamp: number }
```

**WsInputMessage** - User input to terminal:
```typescript
{ type: "input", data: string }
```

**WsStatusMessage** - Task status updates:
```typescript
{ type: "status", status: TaskStatus, exit_code?: number }
```

**WsErrorMessage** - Error notifications:
```typescript
{ type: "error", message: string }
```

**WsMessage** - Discriminated union of all types for type-safe parsing.

## vsock Protocol

JSON-line protocol for host-to-VM communication via vsock.

### Message Types

```typescript
export const VsockMessageType = {
  Init: "init",           // Initialization from host
  Output: "output",       // Terminal output from VM
  Input: "input",         // User input to VM
  Exit: "exit",           // Process exit notification
  Heartbeat: "heartbeat"  // Keep-alive signal
} as const;
```

### Message Schemas

**VsockInitMessage** - Sent from VM API to agent-sidecar:
```typescript
{
  type: "init",
  api_key: string,     // Anthropic API key
  prompt: string,      // User request
  files?: Array<{ name: string, content: string }>
}
```

**VsockOutputMessage** - Terminal output from sidecar:
```typescript
{ type: "output", data: string }
```

**VsockInputMessage** - User input from host:
```typescript
{ type: "input", data: string }
```

**VsockExitMessage** - Process exit notification:
```typescript
{ type: "exit", code: number }
```

## Usage

Import in TypeScript packages:

```typescript
import {
  TaskStatus,
  CreateTaskRequest,
  TaskResponse,
  WsMessage,
  VsockMessage
} from "@lia/shared";
```

## Design Benefits

- **Runtime validation**: Zod validates data at API boundaries
- **Type inference**: TypeScript types generated from schemas
- **Single source of truth**: No duplicate type definitions
- **Cross-package consistency**: Discord bot, web UI, and VM API share types
- **Discriminated unions**: Type-safe message handling
