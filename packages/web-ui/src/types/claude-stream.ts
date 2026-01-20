// Types for Claude CLI stream-json output format

// System init message
export interface SystemInitMessage {
  type: "system";
  subtype: "init";
  session_id: string;
  model: string;
  tools: string[];
  cwd: string;
  permissionMode: string;
}

// Stream events from Anthropic API
export interface StreamEventMessageStart {
  type: "stream_event";
  event: {
    type: "message_start";
    message: {
      id: string;
      model: string;
      role: "assistant";
    };
  };
  session_id: string;
}

export interface StreamEventContentBlockStartText {
  type: "stream_event";
  event: {
    type: "content_block_start";
    index: number;
    content_block: {
      type: "text";
      text: string;
    };
  };
  session_id: string;
}

export interface StreamEventContentBlockStartToolUse {
  type: "stream_event";
  event: {
    type: "content_block_start";
    index: number;
    content_block: {
      type: "tool_use";
      id: string;
      name: string;
      input: Record<string, unknown>;
    };
  };
  session_id: string;
}

export interface StreamEventContentBlockDeltaText {
  type: "stream_event";
  event: {
    type: "content_block_delta";
    index: number;
    delta: {
      type: "text_delta";
      text: string;
    };
  };
  session_id: string;
}

export interface StreamEventContentBlockDeltaJson {
  type: "stream_event";
  event: {
    type: "content_block_delta";
    index: number;
    delta: {
      type: "input_json_delta";
      partial_json: string;
    };
  };
  session_id: string;
}

export interface StreamEventContentBlockStop {
  type: "stream_event";
  event: {
    type: "content_block_stop";
    index: number;
  };
  session_id: string;
}

export interface StreamEventMessageDelta {
  type: "stream_event";
  event: {
    type: "message_delta";
    delta: {
      stop_reason: "end_turn" | "tool_use" | null;
    };
    usage?: {
      output_tokens: number;
    };
  };
  session_id: string;
}

export interface StreamEventMessageStop {
  type: "stream_event";
  event: {
    type: "message_stop";
  };
  session_id: string;
}

// Assistant message with complete content
export interface AssistantMessage {
  type: "assistant";
  message: {
    id: string;
    model: string;
    role: "assistant";
    content: Array<TextContent | ToolUseContent>;
    stop_reason: "end_turn" | "tool_use" | null;
  };
  session_id: string;
}

export interface TextContent {
  type: "text";
  text: string;
}

export interface ToolUseContent {
  type: "tool_use";
  id: string;
  name: string;
  input: Record<string, unknown>;
}

// User message (tool results)
export interface UserToolResultMessage {
  type: "user";
  message: {
    role: "user";
    content: Array<{
      type: "tool_result";
      tool_use_id: string;
      content: string;
      is_error: boolean;
    }>;
  };
  session_id: string;
  tool_use_result?: {
    stdout?: string;
    stderr?: string;
    interrupted?: boolean;
    isImage?: boolean;
  };
}

// Final result
export interface ResultMessage {
  type: "result";
  subtype: "success" | "error";
  is_error: boolean;
  duration_ms: number;
  num_turns: number;
  result: string;
  session_id: string;
  total_cost_usd: number;
  usage: {
    input_tokens: number;
    output_tokens: number;
  };
}

// Union type for all message types
export type ClaudeStreamMessage =
  | SystemInitMessage
  | StreamEventMessageStart
  | StreamEventContentBlockStartText
  | StreamEventContentBlockStartToolUse
  | StreamEventContentBlockDeltaText
  | StreamEventContentBlockDeltaJson
  | StreamEventContentBlockStop
  | StreamEventMessageDelta
  | StreamEventMessageStop
  | AssistantMessage
  | UserToolResultMessage
  | ResultMessage;

// Parsed message types for UI rendering
export interface ParsedTextMessage {
  type: "text";
  id: string;
  content: string;
  isStreaming: boolean;
}

export interface ParsedToolCall {
  type: "tool_call";
  id: string;
  toolName: string;
  input: Record<string, unknown>;
  result?: string;
  isError?: boolean;
  isComplete: boolean;
}

export interface ParsedUserInput {
  type: "user_input";
  id: string;
  content: string;
}

export interface ParsedSystemInfo {
  type: "system_info";
  id: string;
  model: string;
  sessionId: string;
}

export interface ParsedResult {
  type: "result";
  id: string;
  success: boolean;
  durationMs: number;
  costUsd: number;
  numTurns: number;
}

export type ParsedMessage =
  | ParsedTextMessage
  | ParsedToolCall
  | ParsedUserInput
  | ParsedSystemInfo
  | ParsedResult;
