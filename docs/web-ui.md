# Web UI

The Web UI is a React application that provides a real-time terminal interface for interacting with Claude Code agents running in Firecracker VMs.

## Architecture

```
packages/web-ui/
├── src/
│   ├── main.tsx                 # App entry point with routing
│   ├── App.tsx                  # Home/landing page component
│   ├── store.ts                 # Zustand state management
│   ├── api.ts                   # API client and WebSocket creation
│   ├── index.css                # Global styles + xterm customization
│   ├── components/
│   │   ├── Terminal.tsx         # xterm.js terminal wrapper
│   │   ├── TaskHeader.tsx       # Header with task info and controls
│   │   └── InputBar.tsx         # Input textarea for user prompts
│   └── pages/
│       └── TaskPage.tsx         # Main task view page
├── vite.config.ts              # Vite configuration with API proxy
└── package.json
```

## Technology Stack

- **Framework**: React 18.2.0 with React Router v6
- **Build Tool**: Vite 5.0
- **State Management**: Zustand 4.4.0
- **Terminal**: xterm.js 5.3.0 with addons (fit, web-links)
- **Styling**: Tailwind CSS 3.4.0
- **Type Safety**: TypeScript 5.3

## Routing

| Route | Component | Purpose |
|-------|-----------|---------|
| `/` | `App.tsx` | Home/landing page with instructions |
| `/tasks/:taskId` | `TaskPage.tsx` | Main task view with terminal |

## Components

### TaskPage (`src/pages/TaskPage.tsx`)

Main container that orchestrates:
- Task metadata fetching via REST API
- WebSocket connection establishment
- Layout of header, terminal, and input bar
- Conditional rendering based on connection status

### Terminal (`src/components/Terminal.tsx`)

xterm.js wrapper with features:
- Dark VS Code-like theme
- Font: Fira Code/Cascadia Code monospace
- 10,000 line scrollback buffer
- FitAddon for responsive sizing
- WebLinksAddon for clickable URLs
- Incremental output rendering via refs

### TaskHeader (`src/components/TaskHeader.tsx`)

Displays task information and controls:
- Status badge with color coding and pulse animation
- Task ID and truncated prompt
- Resume button (when suspended)
- End Session button with confirmation
- SSH command display with copy-to-clipboard

### InputBar (`src/components/InputBar.tsx`)

User input component:
- Auto-expanding textarea
- Enter to send, Shift+Enter for newlines
- Disabled when task is suspended
- Auto-focuses on mount

## State Management

Zustand store (`src/store.ts`):

```typescript
interface TaskState {
  task: TaskResponse | null;           // Current task metadata
  status: "idle" | "loading" | "error" | "connected";
  error: string | null;
  output: string[];                    // Array of output chunks
  ws: WebSocket | null;                // Active WebSocket connection
}
```

Actions:
- `setTask()` - Update task metadata
- `setStatus()` - Change connection status
- `setError()` - Set error messages
- `appendOutput()` - Add new output chunks
- `clearOutput()` - Reset output array
- `setWebSocket()` - Store WebSocket reference
- `sendInput()` - Send input via WebSocket
- `reset()` - Return to initial state

## WebSocket Communication

### Connection

```typescript
// Auto-detects secure WebSocket for HTTPS
const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
const url = `${protocol}//${host}/api/v1/tasks/${taskId}/stream`;
```

### Message Types

**Received from server:**
- `output`: Terminal output data with timestamp
- `status`: Task status updates (running, suspended, terminated)
- `error`: Error messages

**Sent to server:**
- `input`: User input for Claude Code

### Connection Lifecycle

1. `onopen`: Set status to "connected"
2. `onmessage`: Parse JSON, dispatch to store
3. `onerror`: Set status to "error"
4. `onclose`: Set status to "idle"

## REST API

Endpoints used (`src/api.ts`):

| Method | Endpoint | Purpose |
|--------|----------|---------|
| GET | `/api/v1/tasks/{taskId}` | Fetch task metadata |
| POST | `/api/v1/tasks/{taskId}/resume` | Resume suspended task |
| DELETE | `/api/v1/tasks/{taskId}` | Stop/terminate task |

## Status Display

| Status | Color | Animation |
|--------|-------|-----------|
| Pending | Yellow | - |
| Starting | Blue | - |
| Running | Green | Pulse |
| Suspended | Orange | - |
| Terminated | Gray | - |

## Development

```bash
# Install dependencies
bun install

# Start development server (port 5173)
bun run dev

# Type checking
bun run typecheck

# Build for production
bun run build
```

## Vite Configuration

- Dev server on port 5173
- API proxy: `/api` → `http://localhost:3000` (VM API)
- React Fast Refresh enabled

## Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                       Browser (Web-UI)                       │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  TaskPage (Container)                                         │
│  └─ Fetches task via REST: GET /api/v1/tasks/{id}           │
│  └─ Creates WebSocket: ws://.../api/v1/tasks/{id}/stream    │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │  Zustand Store (useTaskStore)                           │ │
│  │  ├─ task: TaskResponse                                  │ │
│  │  ├─ status: connection status                           │ │
│  │  ├─ output: string[]                                    │ │
│  │  └─ ws: WebSocket                                       │ │
│  └─────────────────────────────────────────────────────────┘ │
│         ↑                                    ↓                │
│  ┌──────────────────┐  ┌──────────────┐  ┌──────────────┐   │
│  │  TaskHeader      │  │  Terminal    │  │  InputBar    │   │
│  │ (Status, SSH)    │  │ (xterm.js)   │  │ (Textarea)   │   │
│  └──────────────────┘  └──────────────┘  └──────────────┘   │
└─────────────────────────────────────────────────────────────┘
       ↕  (REST/WebSocket)
┌─────────────────────────────────────────────────────────────┐
│                    VM API Server (Rust)                      │
└─────────────────────────────────────────────────────────────┘
```
