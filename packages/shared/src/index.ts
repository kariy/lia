import { z } from "zod";

// Task status enum
export const TaskStatus = {
  Pending: "pending",
  Starting: "starting",
  Running: "running",
  Suspended: "suspended",
  Terminated: "terminated",
} as const;

export type TaskStatus = (typeof TaskStatus)[keyof typeof TaskStatus];

// Task source enum
export const TaskSource = {
  Discord: "discord",
  Web: "web",
} as const;

export type TaskSource = (typeof TaskSource)[keyof typeof TaskSource];

// GitHub repository format validation
export const GitHubRepoSchema = z
  .string()
  .regex(
    /^[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+$/,
    "Repository must be in 'owner/repo' format"
  );

// Task configuration schema
export const TaskConfigSchema = z.object({
  timeout_minutes: z.number().optional().default(30),
  max_memory_mb: z.number().optional().default(2048),
  vcpu_count: z.number().optional().default(2),
  storage_gb: z.number().optional().default(50),
});

export type TaskConfig = z.infer<typeof TaskConfigSchema>;

// Task creation request
export const CreateTaskRequestSchema = z.object({
  prompt: z.string().min(1).max(100000),
  repositories: z.array(GitHubRepoSchema).min(1),
  source: z.enum(["discord", "web"]),
  user_id: z.string().min(1).optional(),
  guild_id: z.string().optional(),
  config: TaskConfigSchema.optional(),
  files: z
    .array(
      z.object({
        name: z.string(),
        content: z.string(),
      })
    )
    .optional(),
  ssh_public_key: z.string().optional(),
});

export type CreateTaskRequest = z.infer<typeof CreateTaskRequestSchema>;

// Task response
export const TaskResponseSchema = z.object({
  id: z.string().uuid(),
  user_id: z.string(),
  guild_id: z.string().nullable(),
  status: z.enum(["pending", "starting", "running", "suspended", "terminated"]),
  source: z.enum(["discord", "web"]),
  repositories: z.array(z.string()),
  vm_id: z.string().nullable(),
  config: TaskConfigSchema.nullable(),
  created_at: z.string().datetime(),
  started_at: z.string().datetime().nullable(),
  completed_at: z.string().datetime().nullable(),
  exit_code: z.number().nullable(),
  error_message: z.string().nullable(),
  web_url: z.string().url().optional(),
  ssh_command: z.string().nullable().optional(),
  ip_address: z.string().nullable().optional(),
});

export type TaskResponse = z.infer<typeof TaskResponseSchema>;

// VM boot progress stages (verbose for vm-api, simplified for display)
export const BootStage = {
  CreatingVm: "creating_vm",
  WaitingForSocket: "waiting_for_socket",
  ConfiguringVm: "configuring_vm",
  BootingVm: "booting_vm",
  ConnectingAgent: "connecting_agent",
  InitializingClaude: "initializing_claude",
  Ready: "ready",
} as const;

export type BootStage = (typeof BootStage)[keyof typeof BootStage];

// Human-readable boot stage messages for UI display
export const BootStageMessages: Record<BootStage, string> = {
  creating_vm: "Starting VM...",
  waiting_for_socket: "Starting VM...",
  configuring_vm: "Configuring VM...",
  booting_vm: "Booting...",
  connecting_agent: "Connecting...",
  initializing_claude: "Initializing Claude...",
  ready: "Ready",
};

// WebSocket message types
export const WsMessageType = {
  Output: "output",
  Input: "input",
  Status: "status",
  Progress: "progress",
  Error: "error",
  Ping: "ping",
  Pong: "pong",
} as const;

export type WsMessageType = (typeof WsMessageType)[keyof typeof WsMessageType];

// WebSocket messages
export const WsOutputMessageSchema = z.object({
  type: z.literal("output"),
  data: z.string(),
  timestamp: z.number(),
});

export const WsInputMessageSchema = z.object({
  type: z.literal("input"),
  data: z.string(),
});

export const WsStatusMessageSchema = z.object({
  type: z.literal("status"),
  status: z.enum(["pending", "starting", "running", "suspended", "terminated"]),
  exit_code: z.number().nullable().optional(),
});

export const WsProgressMessageSchema = z.object({
  type: z.literal("progress"),
  stage: z.enum([
    "creating_vm",
    "waiting_for_socket",
    "configuring_vm",
    "booting_vm",
    "connecting_agent",
    "initializing_claude",
    "ready",
  ]),
  message: z.string(), // Human-readable message for UI
});

export const WsErrorMessageSchema = z.object({
  type: z.literal("error"),
  message: z.string(),
});

export const WsPingMessageSchema = z.object({
  type: z.literal("ping"),
});

export const WsPongMessageSchema = z.object({
  type: z.literal("pong"),
});

export const WsMessageSchema = z.discriminatedUnion("type", [
  WsOutputMessageSchema,
  WsInputMessageSchema,
  WsStatusMessageSchema,
  WsProgressMessageSchema,
  WsErrorMessageSchema,
  WsPingMessageSchema,
  WsPongMessageSchema,
]);

export type WsMessage = z.infer<typeof WsMessageSchema>;
export type WsOutputMessage = z.infer<typeof WsOutputMessageSchema>;
export type WsInputMessage = z.infer<typeof WsInputMessageSchema>;
export type WsStatusMessage = z.infer<typeof WsStatusMessageSchema>;
export type WsProgressMessage = z.infer<typeof WsProgressMessageSchema>;
export type WsErrorMessage = z.infer<typeof WsErrorMessageSchema>;

// API error response
export const ApiErrorSchema = z.object({
  error: z.string(),
  code: z.string().optional(),
  details: z.record(z.unknown()).optional(),
});

export type ApiError = z.infer<typeof ApiErrorSchema>;

// Task list response
export const TaskListResponseSchema = z.object({
  tasks: z.array(TaskResponseSchema),
  total: z.number(),
  page: z.number(),
  per_page: z.number(),
});

export type TaskListResponse = z.infer<typeof TaskListResponseSchema>;

// vsock protocol messages (for sidecar communication)
export const VsockMessageType = {
  Init: "init",
  Output: "output",
  Input: "input",
  Exit: "exit",
  Heartbeat: "heartbeat",
} as const;

export type VsockMessageType =
  (typeof VsockMessageType)[keyof typeof VsockMessageType];

export interface VsockInitMessage {
  type: "init";
  api_key: string;
  prompt: string;
  files?: Array<{ name: string; content: string }>;
}

export interface VsockOutputMessage {
  type: "output";
  data: string;
}

export interface VsockInputMessage {
  type: "input";
  data: string;
}

export interface VsockExitMessage {
  type: "exit";
  code: number;
}

export interface VsockHeartbeatMessage {
  type: "heartbeat";
}

export type VsockMessage =
  | VsockInitMessage
  | VsockOutputMessage
  | VsockInputMessage
  | VsockExitMessage
  | VsockHeartbeatMessage;
