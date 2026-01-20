import type { ParsedUserInput } from "../../types/claude-stream";

interface UserInputMessageProps {
  message: ParsedUserInput;
}

export function UserInputMessage({ message }: UserInputMessageProps) {
  return (
    <div className="flex gap-3 justify-end">
      <div className="max-w-[80%] rounded-lg bg-primary px-4 py-2 text-primary-foreground">
        <p className="text-sm whitespace-pre-wrap">{message.content}</p>
      </div>
      <div className="flex-shrink-0 w-6 h-6 rounded-full bg-primary flex items-center justify-center">
        <span className="text-xs font-medium text-primary-foreground">U</span>
      </div>
    </div>
  );
}
