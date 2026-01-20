# Database Schema

This document describes the PostgreSQL database schema used by the VM API service.

## Overview

The database stores task metadata and state for Claude Code agent sessions running in Firecracker microVMs. SQLx is used for compile-time query verification.

## Tables

### tasks

The primary table tracking all Claude Code agent sessions.

| Column | Type | Nullable | Default | Description |
|--------|------|----------|---------|-------------|
| `id` | UUID | NO | - | Primary key, unique task identifier |
| `user_id` | VARCHAR(64) | NO | - | Discord user ID who created the task |
| `status` | VARCHAR(32) | NO | `'pending'` | Current task state (see State Machine below) |
| `vm_id` | VARCHAR(64) | YES | - | Firecracker VM identifier, set when VM is created |
| `ip_address` | VARCHAR(15) | YES | - | VM IP address (e.g., `172.16.0.100`) |
| `config` | JSONB | YES | - | Additional task configuration (model, timeout, etc.) |
| `created_at` | TIMESTAMPTZ | NO | `NOW()` | Timestamp when task was created |
| `started_at` | TIMESTAMPTZ | YES | - | Timestamp when VM started running |
| `completed_at` | TIMESTAMPTZ | YES | - | Timestamp when task finished (success or failure) |
| `exit_code` | INTEGER | YES | - | Claude Code process exit code (0 = success) |
| `error_message` | TEXT | YES | - | Error description if task failed |

**Note:** Prompts and conversation history are stored locally in each VM's persistent storage, not in PostgreSQL, to avoid storing large amounts of conversation data in the database.

#### State Machine

The `status` column follows this state machine:

```
pending → starting → running → suspended
                         ↓         ↓
                    terminated ←───┘
```

| Status | Description |
|--------|-------------|
| `pending` | Task created, waiting to be scheduled |
| `starting` | VM is being provisioned |
| `running` | Claude Code agent is active in the VM |
| `suspended` | VM paused, storage preserved for resume |
| `terminated` | Task completed or failed, VM destroyed |

#### Indexes

| Index Name | Columns | Purpose |
|------------|---------|---------|
| `idx_tasks_user_id` | `user_id` | Find all tasks for a specific user |
| `idx_tasks_status` | `status` | Find tasks by current state |
| `idx_tasks_created_at` | `created_at DESC` | Chronological task listing |
| `idx_tasks_user_status` | `user_id, status` | Find a user's tasks filtered by state |
| `idx_tasks_ip_address` | `ip_address` | Lookup task by VM IP address |

### guild_tasks

Associates tasks with Discord guilds. Tasks created via DM (not in a guild) will not have an entry in this table.

| Column | Type | Nullable | Default | Description |
|--------|------|----------|---------|-------------|
| `task_id` | UUID | NO | - | Primary key, references `tasks(id)` |
| `guild_id` | VARCHAR(64) | NO | - | Discord guild/server snowflake ID |
| `created_at` | TIMESTAMPTZ | NO | `NOW()` | Timestamp when association was created |

#### Indexes

| Index Name | Columns | Purpose |
|------------|---------|---------|
| `idx_guild_tasks_guild_id` | `guild_id` | Find all tasks for a specific guild |
| `idx_guild_tasks_guild_created` | `guild_id, created_at DESC` | Chronological task listing per guild |

## Relationships

```
┌─────────────┐         ┌─────────────┐
│   tasks     │ 1 ─── 0..1 guild_tasks │
│             │◄────────│             │
│ id (PK)     │         │ task_id (PK,FK)
│ user_id     │         │ guild_id    │
│ status      │         │ created_at  │
│ ...         │         └─────────────┘
└─────────────┘
```

- **tasks ← guild_tasks**: One-to-one optional relationship. A task may belong to a guild (via `guild_tasks`) or be a DM task (no entry in `guild_tasks`). The foreign key cascades on delete.

**External references (not enforced by FK constraints):**
- `tasks.user_id` → Discord user snowflake ID
- `guild_tasks.guild_id` → Discord guild snowflake ID
- `tasks.vm_id` → Firecracker VM instance (managed by VM API, not in database)

## Migrations

Migrations are located in `services/vm-api/migrations/` and run via `make db-migrate`.

| Migration | Description |
|-----------|-------------|
| `20240101000000_create_tasks.sql` | Creates the tasks table with core columns and indexes |
| `20240101000001_add_ip_address.sql` | Adds `ip_address` column for VM network tracking |
| `20240101000002_create_guild_tasks.sql` | Creates the guild_tasks table for task-guild associations |

## Usage Patterns

**Create a new task (DM - no guild):**
```sql
INSERT INTO tasks (id, user_id, status)
VALUES ($1, $2, 'pending');
```

**Create a new task (in a guild):**
```sql
INSERT INTO tasks (id, user_id, status)
VALUES ($1, $2, 'pending');

INSERT INTO guild_tasks (task_id, guild_id)
VALUES ($1, $3);
```

**Start a task:**
```sql
UPDATE tasks
SET status = 'starting', vm_id = $2, ip_address = $3, started_at = NOW()
WHERE id = $1;
```

**Complete a task:**
```sql
UPDATE tasks
SET status = 'terminated', completed_at = NOW(), exit_code = $2
WHERE id = $1;
```

**Get user's active tasks:**
```sql
SELECT * FROM tasks
WHERE user_id = $1 AND status IN ('pending', 'starting', 'running', 'suspended')
ORDER BY created_at DESC;
```

**Get all tasks for a guild:**
```sql
SELECT t.* FROM tasks t
JOIN guild_tasks gt ON t.id = gt.task_id
WHERE gt.guild_id = $1
ORDER BY t.created_at DESC;
```

**Get task with guild info (if any):**
```sql
SELECT t.*, gt.guild_id FROM tasks t
LEFT JOIN guild_tasks gt ON t.id = gt.task_id
WHERE t.id = $1;
```
