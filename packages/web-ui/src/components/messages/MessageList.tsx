import { useEffect, useRef, useCallback } from "react";
import { useTaskStore } from "../../store";
import { TextMessage } from "./TextMessage";
import { ToolCallMessage } from "./ToolCallMessage";
import { UserInputMessage } from "./UserInputMessage";
import { ResultMessage } from "./ResultMessage";
import type { ParsedMessage } from "../../types/claude-stream";

export function MessageList() {
  const messages = useTaskStore((state) => state.messages);
  const setIsScrolledToBottom = useTaskStore((state) => state.setIsScrolledToBottom);
  const containerRef = useRef<HTMLDivElement>(null);
  const isAutoScrolling = useRef(false);

  // Check if scrolled to bottom (with small threshold for rounding)
  const checkIfAtBottom = useCallback(() => {
    if (!containerRef.current) return true;
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
    const threshold = 20; // pixels from bottom to consider "at bottom"
    return scrollHeight - scrollTop - clientHeight < threshold;
  }, []);

  // Handle scroll events
  const handleScroll = useCallback(() => {
    if (isAutoScrolling.current) return;
    setIsScrolledToBottom(checkIfAtBottom());
  }, [checkIfAtBottom, setIsScrolledToBottom]);

  // Auto-scroll to bottom when new messages arrive (if already at bottom)
  useEffect(() => {
    if (containerRef.current && checkIfAtBottom()) {
      isAutoScrolling.current = true;
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
      // Reset flag after scroll completes
      requestAnimationFrame(() => {
        isAutoScrolling.current = false;
        setIsScrolledToBottom(true);
      });
    }
  }, [messages, checkIfAtBottom, setIsScrolledToBottom]);

  // Set initial state
  useEffect(() => {
    setIsScrolledToBottom(true);
  }, [setIsScrolledToBottom]);

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
      onScroll={handleScroll}
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
