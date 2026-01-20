# Web UI

The Web UI is a React application that provides a real-time interface for interacting with Claude Code agents running in Firecracker VMs. It renders Claude's stream-json output as structured message components.

## Architecture

```
packages/web-ui/
├── src/
│   ├── main.tsx                 # App entry with routing and toast provider
│   ├── store.ts                 # Zustand state management
│   ├── api.ts                   # REST API client
│   ├── index.css                # Global styles and CSS variables
│   ├── components/
│   │   ├── Layout.tsx           # Sidebar layout with task list
│   │   ├── TaskHeader.tsx       # Header with status, SSH info, controls
│   │   ├── InputBar.tsx         # Follow-up prompt input
│   │   ├── ui/                  # Reusable UI components (shadcn-style)
│   │   └── messages/            # Claude message renderers
│   │       ├── MessageList.tsx  # Scrollable message container
│   │       ├── TextMessage.tsx  # Markdown text rendering
│   │       ├── ToolCallMessage.tsx  # Tool calls with collapsible details
│   │       ├── UserInputMessage.tsx # User follow-up messages
│   │       └── ResultMessage.tsx    # Session result summary
│   ├── lib/
│   │   ├── utils.ts             # Utility functions (cn, etc.)
│   │   └── message-parser.ts    # Claude stream-json parser
│   ├── types/
│   │   └── claude-stream.ts     # TypeScript types for Claude output
│   └── pages/
│       ├── WelcomePage.tsx      # Landing page / new agent form
│       ├── TaskPage.tsx         # Main task view
│       ├── NewAgentPage.tsx     # Create new agent form
│       └── DemoPage.tsx         # Demo with mock data
├── tailwind.config.js           # Tailwind with shadcn theme
├── vite.config.ts               # Vite with API proxy
└── package.json
```

## Technology Stack

- **Framework**: React 18 with React Router v6
- **Build Tool**: Vite 5
- **State Management**: Zustand
- **Styling**: Tailwind CSS with CSS variables (white/gray theme)
- **UI Components**: Radix UI primitives (shadcn-style)
- **Markdown**: react-markdown with remark-gfm
- **Notifications**: sonner (toast notifications)
- **Animations**: tailwindcss-animate
- **Type Safety**: TypeScript 5

## Routing

| Route | Component | Purpose |
|-------|-----------|---------|
| `/` | `WelcomePage` | Landing page, create new agent |
| `/tasks/:taskId` | `TaskPage` | Active task view with messages |
| `/demo` | `DemoPage` | Demo mode with mock Claude output |

All routes use `Layout` which provides a collapsible sidebar with the task list.

## Key Features

### Message Rendering

Instead of a raw terminal, the UI renders Claude's stream-json output as structured components:

- **Text Messages**: Rendered as markdown with syntax highlighting
- **Tool Calls**: Collapsible cards showing tool name, inputs, and outputs
- **User Input**: Displays follow-up prompts from the user
- **Results**: Session summary with cost, duration, and token usage

### Sidebar Layout

- Collapsible sidebar showing all tasks
- Task status indicators with color-coded badges
- Click to navigate between tasks
- "New Agent" button to create tasks

### Task Header

- Status badge with pulse animation for running tasks
- Task ID display
- SSH command with copy-to-clipboard (shows toast notification)
- Resume button (when suspended)
- "More options" dropdown with End Session (requires confirmation)

### Input Bar

- Auto-expanding textarea for follow-up prompts
- Enter to send, Shift+Enter for newlines
- Disabled when task is suspended
- Shadow effect when content is scrollable above

## State Management

Zustand store (`src/store.ts`):

```typescript
interface TaskState {
  task: TaskResponse | null;
  status: "idle" | "loading" | "error" | "connected";
  error: string | null;
  messages: ParsedMessage[];      // Parsed Claude messages
  ws: WebSocket | null;
  parser: MessageParser;          // Stream-json parser instance
  isScrolledToBottom: boolean;    // For input bar shadow
}
```

Key actions:
- `setTask()` - Update task metadata
- `processOutput()` - Parse stream-json line and update messages
- `sendInput()` - Send user input via WebSocket
- `reset()` - Clear state for new task

## Message Parser

The `MessageParser` class (`src/lib/message-parser.ts`) transforms Claude's stream-json output into structured messages:

1. Parses JSON lines from the stream
2. Handles streaming text deltas (builds up text incrementally)
3. Buffers tool call JSON until complete
4. Maintains message state across stream events

Supported message types from Claude CLI:
- `system` - Session initialization info
- `stream_event` - Content blocks (text, tool_use)
- `user` - Tool results
- `result` - Session completion

## WebSocket Communication

### Connection

```typescript
const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
const url = `${protocol}//${host}/api/v1/tasks/${taskId}/stream`;
```

### Message Flow

**Received from server:**
- `output`: Stream-json lines from Claude CLI
- `status`: Task status updates
- `error`: Error messages

**Sent to server:**
- `input`: User follow-up prompts

## REST API

| Method | Endpoint | Purpose |
|--------|----------|---------|
| GET | `/api/v1/tasks` | List all tasks |
| GET | `/api/v1/tasks/{id}` | Get task details |
| POST | `/api/v1/tasks` | Create new task |
| POST | `/api/v1/tasks/{id}/resume` | Resume suspended task |
| DELETE | `/api/v1/tasks/{id}` | Terminate task |

## Styling

The UI uses a white/gray color palette defined via CSS variables:

```css
:root {
  --background: 0 0% 100%;      /* White */
  --foreground: 0 0% 10%;       /* Near black */
  --muted: 0 0% 96%;            /* Light gray */
  --border: 0 0% 90%;           /* Border gray */
  --destructive: 0 72% 51%;     /* Red (for dangerous actions) */
}
```

## Development

```bash
# Start development server (port 5173)
make dev-web
# or
cd packages/web-ui && bun run dev

# Type checking
bun run typecheck

# Build for production
bun run build
```

## Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                       Browser (Web UI)                       │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Layout (Sidebar + Content)                                  │
│  ├─ Sidebar: Task list, navigation                          │
│  └─ Content: TaskPage / WelcomePage / DemoPage              │
│                                                              │
│  TaskPage                                                    │
│  ├─ Fetches task via REST: GET /api/v1/tasks/{id}           │
│  └─ Creates WebSocket: ws://.../api/v1/tasks/{id}/stream    │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Zustand Store                                         │ │
│  │  ├─ task: TaskResponse                                 │ │
│  │  ├─ messages: ParsedMessage[]                          │ │
│  │  ├─ parser: MessageParser                              │ │
│  │  └─ ws: WebSocket                                      │ │
│  └────────────────────────────────────────────────────────┘ │
│         ↑                                    ↓               │
│  ┌──────────────┐  ┌────────────────┐  ┌──────────────┐    │
│  │ TaskHeader   │  │  MessageList   │  │  InputBar    │    │
│  │ (Status,SSH) │  │  (Text,Tools)  │  │  (Textarea)  │    │
│  └──────────────┘  └────────────────┘  └──────────────┘    │
└─────────────────────────────────────────────────────────────┘
       ↕  (REST / WebSocket)
┌─────────────────────────────────────────────────────────────┐
│                    VM API Server (Rust)                      │
│                           ↕                                  │
│                    Claude Code (VM)                          │
└─────────────────────────────────────────────────────────────┘
```
