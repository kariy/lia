# Discord Bot

The Discord bot is the primary user interface for Lia, allowing users to spawn, manage, and interact with Claude Code agents through Discord slash commands.

## Architecture

```
packages/discord-bot/
├── src/
│   ├── index.ts              # Main bot entry point
│   ├── config.ts             # Configuration management
│   ├── api-client.ts         # VM API communication
│   ├── deploy-commands.ts    # Command deployment utility
│   └── commands/
│       ├── index.ts          # Command exports
│       ├── spawn.ts          # Spawn new agent
│       ├── spawn-file.ts     # Spawn with file attachment
│       ├── status.ts         # Check task status
│       ├── resume.ts         # Resume suspended agent
│       ├── stop.ts           # Terminate agent
│       └── list.ts           # List user's agents
├── package.json
└── tsconfig.json
```

## Slash Commands

| Command | Description | Options |
|---------|-------------|---------|
| `/spawn` | Spawn a new Claude Code agent with a text prompt | `prompt` (string, max 4000 chars) |
| `/spawn-file` | Spawn agent with file attachment for context | `prompt` (string), `file` (attachment, max 10MB) |
| `/status` | Query status of a running/suspended agent | `task_id` (string, required) |
| `/resume` | Resume a suspended agent to continue work | `task_id` (string, required) |
| `/stop` | Terminate agent and free all resources | `task_id` (string, required) |
| `/list` | Show user's active/suspended agents | `status` (all/running/suspended/pending, optional) |

## Configuration

Environment variables required:

| Variable | Description | Required | Default |
|----------|-------------|----------|---------|
| `DISCORD_TOKEN` | Bot authentication token | Yes | - |
| `DISCORD_CLIENT_ID` | Application ID for slash commands | Yes | - |
| `VM_API_URL` | VM API server URL | No | `http://localhost:3000` |
| `WEB_URL` | Web UI URL for task links | No | `http://localhost:5173` |

Configuration is validated using Zod schemas at startup (`src/config.ts`).

## API Client

The `VmApiClient` class (`src/api-client.ts`) provides communication with the VM API:

| Method | Endpoint | Purpose |
|--------|----------|---------|
| `createTask()` | `POST /api/v1/tasks` | Create new task with prompt/files |
| `getTask()` | `GET /api/v1/tasks/{taskId}` | Fetch single task by ID |
| `listTasks()` | `GET /api/v1/tasks` | Paginated task listing with filters |
| `resumeTask()` | `POST /api/v1/tasks/{taskId}/resume` | Resume suspended task |
| `deleteTask()` | `DELETE /api/v1/tasks/{taskId}` | Terminate task |

Request and response types are imported from `@lia/shared` for type safety.

## Task Status Display

Commands use consistent formatting for task status:

| Status | Emoji | Color |
|--------|-------|-------|
| Pending | ⏳ | Orange |
| Starting | 🚀 | Blue |
| Running | ▶️ | Green |
| Suspended | ⏸️ | Yellow |
| Terminated | ⏹️ | Gray |

## Response Features

- **Rich Embeds**: All responses use Discord embeds with color coding
- **Deferred Replies**: Commands defer replies to handle API latency
- **Task Links**: Responses include web URL for browser access
- **SSH Info**: Running tasks show SSH command when available
- **Pagination**: List command shows up to 10 tasks with total count

## File Handling

The `/spawn-file` command:
1. Downloads attachment from Discord CDN
2. Validates file size (max 10MB)
3. Converts content to string
4. Passes as array of file objects to API

## Development

```bash
# Install dependencies
bun install

# Run in development mode
bun run dev

# Deploy slash commands to Discord
bun run deploy-commands

# Type checking
bun run typecheck

# Build for production
bun run build
```

## User Association

All tasks are associated with:
- `user_id`: Discord user ID (used for filtering in `/list`)
- `guild_id`: Discord server ID (optional, for future multi-tenant features)
