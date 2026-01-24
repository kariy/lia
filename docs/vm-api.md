# VM API

The VM API is a Rust web service built with Axum that manages QEMU VMs for Claude Code execution. It handles task lifecycle, VM orchestration, WebSocket streaming, and database persistence.

## Architecture

```
services/vm-api/
├── src/
│   ├── main.rs           # Application bootstrap
│   ├── config.rs         # Configuration management
│   ├── models.rs         # Data structures
│   ├── db.rs             # Database operations
│   ├── handlers.rs       # HTTP endpoint handlers
│   ├── qemu.rs           # VM lifecycle management
│   ├── vsock.rs          # Host-to-VM communication
│   ├── ws.rs             # WebSocket registry
│   └── error.rs          # Error handling
├── config/
│   ├── default.toml      # Default configuration
│   └── local.toml.example  # Template for local overrides
├── migrations/           # SQLx migrations
└── Cargo.toml
```

## Technology Stack

- **Framework**: Axum with WebSocket support
- **Database**: PostgreSQL via SQLx (compile-time verification)
- **Runtime**: Tokio async runtime
- **Logging**: Tracing with structured output

## API Endpoints

| Endpoint | Method | Handler | Purpose |
|----------|--------|---------|---------|
| `/health` | GET | `health_check` | Health check |
| `/api/v1/tasks` | POST | `create_task` | Create new task |
| `/api/v1/tasks` | GET | `list_tasks` | List tasks with pagination |
| `/api/v1/tasks/:id` | GET | `get_task` | Get task details |
| `/api/v1/tasks/:id` | DELETE | `delete_task` | Delete task and stop VM |
| `/api/v1/tasks/:id/resume` | POST | `resume_task` | Resume suspended VM |
| `/api/v1/tasks/:id/output` | GET | `get_task_output` | Get buffered output |
| `/api/v1/tasks/:id/stream` | GET | `ws_stream` | WebSocket streaming |

## API Endpoint Details

### GET /health

Simple health check endpoint.

**Response:** `200 OK` with body `"OK"`

**Database Access:** None

---

### POST /api/v1/tasks

Creates a new task and spawns a Firecracker VM.

**Request Body:**
```json
{
  "prompt": "string (required)",
  "user_id": "string (required)",
  "guild_id": "string (optional)",
  "config": {
    "timeout_minutes": 30,
    "max_memory_mb": 2048,
    "vcpu_count": 2,
    "storage_gb": 50
  },
  "files": [
    { "name": "filename", "content": "file content" }
  ],
  "ssh_public_key": "string (optional)"
}
```

**Response:** `200 OK` with `TaskResponse`

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | INSERT | Create task with `status='pending'` |
| 2 | `guild_tasks` | INSERT | Create guild association (if `guild_id` provided) |
| 3 | `tasks` | UPDATE | Set `status='starting'`, `vm_id` |
| 4 | `tasks` | SELECT | Fetch task for response |
| 5 | `guild_tasks` | SELECT | Fetch guild_id for response |

**Background Processing (after response):**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 6 | `tasks` | UPDATE | Set `status='running'`, update `vm_id` |
| 7 | `tasks` | UPDATE | Set `ip_address` |
| 8 (on error) | `tasks` | UPDATE | Set `status='terminated'`, `exit_code=1`, `error_message` |

**Flow Diagram:**
```
Request
   │
   ▼
Validate prompt not empty
   │
   ▼
INSERT INTO tasks ──────────────────┐
   │                                │
   ▼                                │
INSERT INTO guild_tasks (if guild)  │
   │                                │
   ▼                                │
UPDATE tasks (status='starting')    │
   │                                │
   ▼                                │
Spawn background task ──────────────┼──► Return TaskResponse
   │                                     (status='starting')
   ▼
Create Firecracker VM
   │
   ├─── Success ───► UPDATE tasks (status='running', ip_address)
   │                        │
   │                        ▼
   │                 Start vsock relay
   │                        │
   │                        ├─── Success ───► VM running
   │                        │
   │                        └─── Failure ───► UPDATE tasks (terminated, error)
   │
   └─── Failure ───► UPDATE tasks (terminated, error)
```

**Errors:**
- `400 Bad Request`: Prompt is empty
- `500 Database Error`: Database operation failed

---

### GET /api/v1/tasks/:id

Retrieves details of a specific task.

**Path Parameters:**
- `id`: Task UUID

**Response:** `200 OK` with `TaskResponse`

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | SELECT | Fetch task by ID |
| 2 | `guild_tasks` | SELECT | Fetch guild_id for task |

**Flow Diagram:**
```
Request (task_id)
   │
   ▼
SELECT FROM tasks WHERE id = $1
   │
   ├─── Not Found ───► 404 TaskNotFound
   │
   ▼
SELECT guild_id FROM guild_tasks WHERE task_id = $1
   │
   ▼
Return TaskResponse
```

**Errors:**
- `404 Task Not Found`: Task with given ID does not exist

---

### GET /api/v1/tasks

Lists tasks with optional filtering and pagination.

**Query Parameters:**
- `user_id` (optional): Filter by user
- `status` (optional): Filter by status (`pending`, `starting`, `running`, `suspended`, `terminated`)
- `page` (default: 1): Page number
- `per_page` (default: 20): Items per page

**Response:** `200 OK` with `TaskListResponse`
```json
{
  "tasks": [TaskResponse],
  "total": 100,
  "page": 1,
  "per_page": 20
}
```

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | SELECT | Fetch paginated tasks with filters |
| 2 | `tasks` | SELECT COUNT | Get total count for pagination |
| 3 | `guild_tasks` | SELECT (per task) | Fetch guild_id for each task |

**SQL Queries:**
```sql
-- Fetch tasks
SELECT * FROM tasks
WHERE ($1::VARCHAR IS NULL OR user_id = $1)
  AND ($2::VARCHAR IS NULL OR status = $2)
ORDER BY created_at DESC
LIMIT $3 OFFSET $4;

-- Count total
SELECT COUNT(*) FROM tasks
WHERE ($1::VARCHAR IS NULL OR user_id = $1)
  AND ($2::VARCHAR IS NULL OR status = $2);

-- For each task
SELECT guild_id FROM guild_tasks WHERE task_id = $1;
```

---

### DELETE /api/v1/tasks/:id

Stops the VM and marks task as terminated.

**Path Parameters:**
- `id`: Task UUID

**Response:** `204 No Content`

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | SELECT | Fetch task to get vm_id |
| 2 | `tasks` | UPDATE | Set `status='terminated'` |

**Flow Diagram:**
```
Request (task_id)
   │
   ▼
SELECT FROM tasks WHERE id = $1
   │
   ├─── Not Found ───► 404 TaskNotFound
   │
   ▼
Stop VM (if vm_id exists)
   │
   ▼
UPDATE tasks SET status='terminated' WHERE id = $1
   │
   ▼
Remove WebSocket channel from registry
   │
   ▼
Return 204 No Content
```

**Note:** The `guild_tasks` entry is automatically deleted via `ON DELETE CASCADE` when the task is deleted.

**Errors:**
- `404 Task Not Found`: Task with given ID does not exist

---

### POST /api/v1/tasks/:id/resume

Resumes a suspended VM.

**Path Parameters:**
- `id`: Task UUID

**Response:** `200 OK` with `TaskResponse`

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | SELECT | Fetch task, verify status is `suspended` |
| 2 | `tasks` | UPDATE | Set `status='running'` |
| 3 | `guild_tasks` | SELECT | Fetch guild_id for response |

**Flow Diagram:**
```
Request (task_id)
   │
   ▼
SELECT FROM tasks WHERE id = $1
   │
   ├─── Not Found ───► 404 TaskNotFound
   │
   ▼
Check status == 'suspended'
   │
   ├─── Not Suspended ───► 409 InvalidState
   │
   ▼
Check vm_id exists
   │
   ├─── No VM ───► 409 InvalidState
   │
   ▼
Resume VM via VmManager
   │
   ▼
UPDATE tasks SET status='running' WHERE id = $1
   │
   ▼
SELECT guild_id FROM guild_tasks
   │
   ▼
Return TaskResponse
```

**Errors:**
- `404 Task Not Found`: Task does not exist
- `409 Invalid State`: Task is not in suspended state or has no VM

---

### GET /api/v1/tasks/:id/output

Returns buffered terminal output for a task.

**Path Parameters:**
- `id`: Task UUID

**Response:** `200 OK` with array of `WsMessage`

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | SELECT | Verify task exists |

**Flow Diagram:**
```
Request (task_id)
   │
   ▼
SELECT FROM tasks WHERE id = $1
   │
   ├─── Not Found ───► 404 TaskNotFound
   │
   ▼
Get TaskChannel from WsRegistry
   │
   ├─── Not Found ───► Return empty array []
   │
   ▼
Return buffered output messages
```

**Errors:**
- `404 Task Not Found`: Task does not exist

---

### GET /api/v1/tasks/:id/stream (WebSocket)

Establishes WebSocket connection for real-time terminal streaming.

**Path Parameters:**
- `id`: Task UUID

**WebSocket Messages (Server → Client):**
```json
{ "type": "output", "data": "terminal output", "timestamp": 1234567890 }
{ "type": "status", "status": "running", "exit_code": null }
{ "type": "error", "message": "error description" }
{ "type": "pong" }
```

**WebSocket Messages (Client → Server):**
```json
{ "type": "input", "data": "user input" }
{ "type": "ping" }
```

**Database Access:**

| Step | Table | Operation | Description |
|------|-------|-----------|-------------|
| 1 | `tasks` | SELECT | Verify task exists on connection |

**Flow Diagram:**
```
WebSocket Upgrade Request
   │
   ▼
SELECT FROM tasks WHERE id = $1
   │
   ├─── Not Found ───► Close connection
   │
   ▼
Get or create TaskChannel
   │
   ▼
Send buffered output to client
   │
   ▼
Subscribe to broadcast channel
   │
   ▼
┌─────────────────────────────────────┐
│         Bidirectional Loop          │
│                                     │
│  Server → Client: Output, Status    │
│  Client → Server: Input, Ping       │
│                                     │
└─────────────────────────────────────┘
   │
   ▼
On disconnect: cleanup
```

## Task State Machine

```
pending → starting → running → suspended
                         ↓         ↓
                    terminated ←───┘
```

| State | Description |
|-------|-------------|
| `pending` | Task created, waiting to start |
| `starting` | VM is being created |
| `running` | VM running, agent active |
| `suspended` | VM paused, storage preserved |
| `terminated` | Task complete or deleted |

## Response Schemas

### TaskResponse

Returned by task creation, retrieval, and resume endpoints.

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "user_id": "123456789012345678",
  "guild_id": "987654321098765432",
  "status": "running",
  "vm_id": "vm-550e8400-e29b-41d4-a716-446655440000",
  "config": {
    "timeout_minutes": 30,
    "max_memory_mb": 2048,
    "vcpu_count": 2,
    "storage_gb": 50
  },
  "created_at": "2024-01-15T10:30:00Z",
  "started_at": "2024-01-15T10:30:05Z",
  "completed_at": null,
  "exit_code": null,
  "error_message": null,
  "web_url": "http://localhost:5173/tasks/550e8400-e29b-41d4-a716-446655440000",
  "ssh_command": "ssh root@172.16.0.100",
  "ip_address": "172.16.0.100"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID | Unique task identifier |
| `user_id` | string | Discord user ID who created the task |
| `guild_id` | string? | Discord guild ID (null for DM tasks) |
| `status` | string | Current task state |
| `vm_id` | string? | Firecracker VM identifier |
| `config` | object? | Task configuration |
| `created_at` | datetime | When task was created |
| `started_at` | datetime? | When VM started running |
| `completed_at` | datetime? | When task finished |
| `exit_code` | int? | Process exit code (0 = success) |
| `error_message` | string? | Error description if failed |
| `web_url` | string | URL to web UI for this task |
| `ssh_command` | string? | SSH command to connect to VM |
| `ip_address` | string? | VM IP address |

### WsMessage

WebSocket message format for terminal streaming.

**Output Message (Server → Client):**
```json
{
  "type": "output",
  "data": "terminal output text",
  "timestamp": 1705312200000
}
```

**Status Message (Server → Client):**
```json
{
  "type": "status",
  "status": "terminated",
  "exit_code": 0
}
```

**Error Message (Server → Client):**
```json
{
  "type": "error",
  "message": "Connection lost"
}
```

**Input Message (Client → Server):**
```json
{
  "type": "input",
  "data": "user typed text"
}
```

**Ping/Pong (Both directions):**
```json
{ "type": "ping" }
{ "type": "pong" }
```

## Configuration

Config files only (highest to lowest priority):
1. `config/local.toml` - User overrides (not committed)
2. `config/default.toml` - Default values

To get started, copy `config/local.toml.example` to `config/local.toml` and set the required values.

### Configuration Sections

**ServerConfig**:
- `host`: Bind address (default: "0.0.0.0")
- `port`: HTTP port (default: 8811)
- `web_url`: Web UI URL (default: "http://localhost:5173")

**DatabaseConfig**:
- `url`: PostgreSQL connection string (required)
- `max_connections`: Pool size (default: 10)

**QemuConfig**:
- `bin_path`: QEMU binary (default: "/usr/bin/qemu-system-x86_64")
- `kernel_path`: VM kernel (default: "/var/lib/lia/kernel/vmlinuz")
- `rootfs_path`: Base rootfs (default: "/var/lib/lia/rootfs/rootfs.ext4")
- `volumes_dir`: VM volumes (default: "/var/lib/lia/volumes")
- `sockets_dir`: QMP sockets (default: "/var/lib/lia/sockets")
- `logs_dir`: VM logs (default: "/var/lib/lia/logs")
- `pids_dir`: PID files (default: "/var/run/lia")
- `machine_type`: QEMU machine type (default: "q35")

**VmConfig**:
- `default_vcpu_count`: CPU cores (default: 2)
- `default_memory_mb`: RAM (default: 2048)
- `default_storage_gb`: Disk (default: 50)
- `idle_timeout_minutes`: Timeout (default: 30)
- `vsock_cid_start`: Initial CID (default: 100)

**NetworkConfig**:
- `bridge_name`: Network bridge (default: "lia-br0")
- `bridge_ip`: Bridge IP (default: "172.16.0.1")
- `subnet`: VM subnet (default: "172.16.0.0/24")

**ClaudeConfig**:
- `api_key`: Anthropic API key (required)

## Firecracker VM Management

### VM Creation Flow

1. **Allocation**: Assign vsock CID and IP address
2. **Preparation**: Create directories, TAP device, sparse volume, copy rootfs
3. **Log File Creation**: Create empty log file (Firecracker requires it to exist)
4. **Process Start**: Spawn Firecracker with Unix socket
5. **Socket Ready**: Wait for API socket (5s timeout)
6. **Configuration**: Configure via Firecracker HTTP API
7. **Storage**: Store VmInfo in memory

> **Important**: Firecracker requires the log file specified by `--log-path` to exist before the process starts. If missing, Firecracker exits immediately with "Failed to open target file" and the API socket is never created.

### VM Configuration (via Firecracker API)

1. **Boot Source**: Kernel path, boot args with network config
2. **Machine Config**: vcpu_count, mem_size_mib
3. **Storage**: Root drive (rootfs), data drive (volume)
4. **Network**: eth0 with TAP device
5. **vsock**: Guest CID for host-VM communication
6. **Instance Start**: Boot the VM

### IP Address Allocation

- Range: 172.16.0.100-254
- Atomic counter for allocation
- Format: `172.16.0.{counter}`

### TAP Device Management

Helper scripts:
- `lia-create-tap {name} {bridge}`: Create TAP attached to bridge
- `lia-delete-tap {name}`: Delete TAP device

## WebSocket Streaming

### WsRegistry

Central registry mapping task_id to TaskChannel:
- Thread-safe with `Arc<RwLock>`
- Broadcast channels with 1024 capacity
- Output buffering for reconnection

### TaskChannel

- `sender`: broadcast::Sender for multi-subscriber
- `output_buffer`: Message history for new connections

### WebSocket Handler Flow

1. Validate task exists in database
2. Split socket into sender/receiver
3. Get or create TaskChannel
4. Flush buffered output to new client
5. Subscribe to broadcast channel
6. Spawn task to forward messages
7. Handle incoming Input/Ping messages
8. Cleanup on disconnect

## vsock Communication

### Firecracker vsock Protocol

Firecracker exposes a Unix Domain Socket (UDS) for vsock multiplexing. The host must follow a specific protocol for **host-initiated** connections:

1. **Connect** to the vsock UDS (`{sockets_dir}/{vm_id}.vsock`)
2. **Send** `CONNECT <port>\n` to request connection to guest port
3. **Read** response - `OK <host_port>\n` on success
4. **Stream** bidirectional data over the established connection

> **Important**: Simply connecting to the UDS without sending `CONNECT <port>\n` will fail. The guest must be listening on the specified port before the host sends the CONNECT command.

### VsockRelay

Manages bidirectional communication with VM:

1. **Connect**: Retry connection to vsock UDS (10s timeout, 100 attempts)
2. **Handshake**: Send `CONNECT 5000\n`, wait for `OK` response
3. **Initialize**: Send Init message with API key, prompt, files
4. **Reader Task**: Parse VsockMessages, forward to WebSocket
5. **Writer Task**: Receive user input, forward to VM

### Message Protocol

JSON-line format: `<json object>\n`

| Message | Direction | Purpose |
|---------|-----------|---------|
| `Init` | Host → VM | Send API key, prompt, files |
| `Output` | VM → Host | Terminal output |
| `Input` | Host → VM | User input |
| `Exit` | VM → Host | Process exit with code |
| `Heartbeat` | Both | Keep-alive signal |

## Database Schema

See [docs/database.md](./database.md) for the full database schema documentation.

### Tasks Table

```sql
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    user_id VARCHAR(64) NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'pending',
    vm_id VARCHAR(64),
    config JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    exit_code INTEGER,
    error_message TEXT,
    ip_address VARCHAR(15)
);
```

**Note:** Prompts and conversation history are stored locally in each VM's persistent storage, not in PostgreSQL.

### Guild Tasks Table

```sql
CREATE TABLE guild_tasks (
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    guild_id VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (task_id)
);
```

### Indexes

- `idx_tasks_user_id`: Filter by user
- `idx_tasks_status`: Filter by status
- `idx_tasks_created_at`: Sort by creation
- `idx_tasks_user_status`: Combined filter
- `idx_tasks_ip_address`: Lookup by IP address
- `idx_guild_tasks_guild_id`: Filter by guild
- `idx_guild_tasks_guild_created`: Guild tasks by creation time

### Database Functions

| Function | Purpose |
|----------|---------|
| `create_task` | Insert new task with Pending status |
| `create_guild_task` | Associate task with a guild |
| `get_task` | Fetch task by UUID |
| `get_guild_id_for_task` | Get guild ID for a task (if any) |
| `list_tasks` | Paginated list with filters |
| `update_task_status` | Update status and vm_id |
| `update_task_ip_address` | Set VM IP address |
| `complete_task` | Set terminated status, exit code, error |
| `delete_task` | Remove task record |

## Error Handling

| Error | HTTP Status | Description |
|-------|-------------|-------------|
| `TaskNotFound` | 404 | Task does not exist |
| `BadRequest` | 400 | Invalid request data |
| `Unauthorized` | 401 | Authentication failed |
| `VmError` | 500 | VM operation failed |
| `DatabaseError` | 500 | Database operation failed |
| `InvalidState` | 409 | Invalid state transition |

## Development

```bash
# Run in development mode
cargo run

# Run with hot reload
cargo watch -x run

# Run database migrations
sqlx migrate run

# Type checking
cargo check

# Build for production
cargo build --release
```

## Application State

Shared state via `Arc<AppState>`:
- `db`: Database pool
- `config`: Application configuration
- `vm_manager`: VmManager for VM operations
- `ws_registry`: WsRegistry for WebSocket channels
