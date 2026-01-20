import { useState, useRef, useEffect } from "react";
import { useTaskStore } from "../store";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Send } from "lucide-react";
import { cn } from "@/lib/utils";

export function InputBar() {
  const [input, setInput] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const sendInput = useTaskStore((state) => state.sendInput);
  const task = useTaskStore((state) => state.task);
  const isScrolledToBottom = useTaskStore((state) => state.isScrolledToBottom);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.trim()) return;

    sendInput(input + "\n");
    setInput("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  };

  // Focus input on mount
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const isDisabled = task?.status === "suspended";

  return (
    <form
      onSubmit={handleSubmit}
      className={cn(
        "border-t bg-background p-4 transition-shadow duration-200",
        !isScrolledToBottom && "shadow-[0_-4px_12px_rgba(0,0,0,0.08)]"
      )}
    >
      <div className="flex gap-2">
        <Textarea
          ref={inputRef}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={isDisabled}
          placeholder={
            isDisabled
              ? "Resume the session to send input..."
              : "Send a follow-up prompt... (Enter to send, Shift+Enter for newline)"
          }
          className="flex-1 resize-none min-h-[40px] max-h-[120px]"
          rows={1}
        />
        <Button
          type="submit"
          disabled={isDisabled || !input.trim()}
        >
          <Send className="h-4 w-4 mr-2" />
          Send
        </Button>
      </div>
    </form>
  );
}
