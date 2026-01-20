import { useEffect, useRef } from "react";
import { useTaskStore } from "../../store";
import { TextMessage } from "./TextMessage";
import { ToolCallMessage } from "./ToolCallMessage";
import { UserInputMessage } from "./UserInputMessage";
import { ResultMessage } from "./ResultMessage";
import type { ParsedMessage } from "../../types/claude-stream";

export function MessageList() {
  const messages = useTaskStore((state) => state.messages);
  const containerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    if (containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [messages]);

  if (messages.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-muted-foreground">
        Waiting for agent output...
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="h-full overflow-y-auto overflow-x-hidden p-4 space-y-4"
    >
      {messages.map((message) => (
        <MessageItem key={message.id} message={message} />
      ))}
    </div>
  );
}

function MessageItem({ message }: { message: ParsedMessage }) {
  switch (message.type) {
    case "text":
      return <TextMessage message={message} />;
    case "tool_call":
      return <ToolCallMessage message={message} />;
    case "user_input":
      return <UserInputMessage message={message} />;
    case "result":
      return <ResultMessage message={message} />;
    case "system_info":
      // Hide system info messages from UI
      return null;
    default:
      return null;
  }
}
