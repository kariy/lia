import type {
  ClaudeStreamMessage,
  ParsedMessage,
  ParsedTextMessage,
  ParsedToolCall,
  ParsedSystemInfo,
  ParsedResult,
} from "../types/claude-stream";

/**
 * Parses Claude stream-json messages and maintains state for building
 * a list of parsed messages for UI rendering.
 */
export class MessageParser {
  private messages: ParsedMessage[] = [];
  private currentTextMessage: ParsedTextMessage | null = null;
  private currentToolCall: ParsedToolCall | null = null;
  private toolCallJsonBuffer: string = "";
  private messageIdCounter = 0;

  private generateId(): string {
    return `msg-${++this.messageIdCounter}`;
  }

  /**
   * Parse a raw JSON line from Claude's stream-json output
   */
  parseMessage(jsonLine: string): ParsedMessage[] {
    try {
      const msg = JSON.parse(jsonLine) as ClaudeStreamMessage;
      return this.processMessage(msg);
    } catch {
      // Ignore parse errors for malformed JSON
      return [];
    }
  }

  private processMessage(msg: ClaudeStreamMessage): ParsedMessage[] {
    const newMessages: ParsedMessage[] = [];

    switch (msg.type) {
      case "system":
        if (msg.subtype === "init") {
          const sysInfo: ParsedSystemInfo = {
            type: "system_info",
            id: this.generateId(),
            model: msg.model,
            sessionId: msg.session_id,
          };
          this.messages.push(sysInfo);
          newMessages.push(sysInfo);
        }
        break;

      case "stream_event":
        this.handleStreamEvent(msg, newMessages);
        break;

      case "assistant":
        // Assistant message is a snapshot - we can use it to finalize current state
        this.finalizeCurrentContent();
        break;

      case "user":
        // Tool result
        if (msg.message.content) {
          for (const content of msg.message.content) {
            if (content.type === "tool_result") {
              // Find matching tool call and update it
              const toolCall = this.messages.find(
                (m): m is ParsedToolCall =>
                  m.type === "tool_call" && m.id === content.tool_use_id
              );
              if (toolCall) {
                toolCall.result = content.content;
                toolCall.isError = content.is_error;
                toolCall.isComplete = true;
              }
            }
          }
        }
        break;

      case "result":
        this.finalizeCurrentContent();
        const result: ParsedResult = {
          type: "result",
          id: this.generateId(),
          success: !msg.is_error,
          durationMs: msg.duration_ms,
          costUsd: msg.total_cost_usd,
          numTurns: msg.num_turns,
        };
        this.messages.push(result);
        newMessages.push(result);
        break;
    }

    return newMessages;
  }

  private handleStreamEvent(msg: ClaudeStreamMessage, newMessages: ParsedMessage[]) {
    if (msg.type !== "stream_event") return;

    const event = msg.event;

    switch (event.type) {
      case "content_block_start":
        if (event.content_block.type === "text") {
          // Start new text message
          this.finalizeCurrentContent();
          this.currentTextMessage = {
            type: "text",
            id: this.generateId(),
            content: event.content_block.text || "",
            isStreaming: true,
          };
          this.messages.push(this.currentTextMessage);
          newMessages.push(this.currentTextMessage);
        } else if (event.content_block.type === "tool_use") {
          // Start new tool call
          this.finalizeCurrentContent();
          this.toolCallJsonBuffer = "";
          this.currentToolCall = {
            type: "tool_call",
            id: event.content_block.id,
            toolName: event.content_block.name,
            input: event.content_block.input || {},
            isComplete: false,
          };
          this.messages.push(this.currentToolCall);
          newMessages.push(this.currentToolCall);
        }
        break;

      case "content_block_delta":
        if (event.delta.type === "text_delta" && this.currentTextMessage) {
          this.currentTextMessage.content += event.delta.text;
        } else if (event.delta.type === "input_json_delta" && this.currentToolCall) {
          this.toolCallJsonBuffer += event.delta.partial_json;
        }
        break;

      case "content_block_stop":
        if (this.currentToolCall && this.toolCallJsonBuffer) {
          try {
            this.currentToolCall.input = JSON.parse(this.toolCallJsonBuffer);
          } catch {
            // Keep partial input if parse fails
          }
          this.toolCallJsonBuffer = "";
        }
        break;

      case "message_delta":
        // Message is ending
        if (event.delta.stop_reason === "end_turn") {
          this.finalizeCurrentContent();
        }
        break;

      case "message_stop":
        this.finalizeCurrentContent();
        break;
    }
  }

  private finalizeCurrentContent() {
    if (this.currentTextMessage) {
      this.currentTextMessage.isStreaming = false;
      this.currentTextMessage = null;
    }
    if (this.currentToolCall) {
      this.currentToolCall = null;
      this.toolCallJsonBuffer = "";
    }
  }

  /**
   * Add a user input message
   */
  addUserInput(content: string): ParsedMessage {
    const msg: ParsedMessage = {
      type: "user_input",
      id: this.generateId(),
      content,
    };
    this.messages.push(msg);
    return msg;
  }

  /**
   * Get all parsed messages
   */
  getMessages(): ParsedMessage[] {
    return [...this.messages];
  }

  /**
   * Clear all messages
   */
  clear() {
    this.messages = [];
    this.currentTextMessage = null;
    this.currentToolCall = null;
    this.toolCallJsonBuffer = "";
    this.messageIdCounter = 0;
  }
}
