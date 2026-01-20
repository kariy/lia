import { useState } from "react";
import type { ParsedToolCall } from "../../types/claude-stream";
import { cn } from "@/lib/utils";
import { ChevronDown, ChevronRight, Terminal, FileText, Search, Globe, Loader2, Check, X } from "lucide-react";

interface ToolCallMessageProps {
  message: ParsedToolCall;
}

const toolIcons: Record<string, React.ReactNode> = {
  Bash: <Terminal className="h-3.5 w-3.5" />,
  Read: <FileText className="h-3.5 w-3.5" />,
  Write: <FileText className="h-3.5 w-3.5" />,
  Edit: <FileText className="h-3.5 w-3.5" />,
  Glob: <Search className="h-3.5 w-3.5" />,
  Grep: <Search className="h-3.5 w-3.5" />,
  WebFetch: <Globe className="h-3.5 w-3.5" />,
  WebSearch: <Globe className="h-3.5 w-3.5" />,
};

export function ToolCallMessage({ message }: ToolCallMessageProps) {
  const [isExpanded, setIsExpanded] = useState(true);
  const [showResult, setShowResult] = useState(false);

  const icon = toolIcons[message.toolName] || <Terminal className="h-3.5 w-3.5" />;
  const isRunning = !message.isComplete;

  // Format tool input for display
  const getToolSummary = () => {
    const input = message.input;

    switch (message.toolName) {
      case "Bash":
        return String(input.command || "Running command...");
      case "Read":
        return String(input.file_path || "Reading file...");
      case "Write":
        return String(input.file_path || "Writing file...");
      case "Edit":
        return String(input.file_path || "Editing file...");
      case "Glob":
        return String(input.pattern || "Searching files...");
      case "Grep":
        return String(input.pattern || "Searching content...");
      case "WebFetch":
        return String(input.url || "Fetching URL...");
      case "WebSearch":
        return String(input.query || "Searching web...");
      default:
        return JSON.stringify(input).slice(0, 100);
    }
  };

  return (
    <div className="border rounded-lg bg-secondary/30 overflow-hidden">
      {/* Tool header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="flex items-center gap-2 w-full px-3 py-2 text-left hover:bg-secondary/50 transition-colors"
      >
        <span className="text-muted-foreground">
          {isExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        </span>
        <span className="text-muted-foreground">{icon}</span>
        <span className="font-medium text-sm text-foreground">{message.toolName}</span>
        <span className="flex-1 truncate text-sm text-muted-foreground font-mono">
          {getToolSummary()}
        </span>
        {isRunning ? (
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        ) : message.isError ? (
          <X className="h-4 w-4 text-destructive" />
        ) : (
          <Check className="h-4 w-4 text-green-600" />
        )}
      </button>

      {/* Expanded content */}
      {isExpanded && (
        <div className="border-t">
          {/* Input details */}
          <div className="px-3 py-2 bg-background/50">
            <ToolInput toolName={message.toolName} input={message.input} />
          </div>

          {/* Result */}
          {message.result && (
            <div className="border-t">
              <button
                onClick={() => setShowResult(!showResult)}
                className="flex items-center gap-2 w-full px-3 py-1.5 text-left hover:bg-secondary/30 transition-colors"
              >
                <span className="text-xs text-muted-foreground">
                  {showResult ? "Hide" : "Show"} output
                </span>
                {showResult ? <ChevronDown className="h-3 w-3 text-muted-foreground" /> : <ChevronRight className="h-3 w-3 text-muted-foreground" />}
              </button>
              {showResult && (
                <div className={cn(
                  "px-3 py-2 font-mono text-xs overflow-x-auto max-h-64 overflow-y-auto",
                  message.isError ? "bg-destructive/10 text-destructive" : "bg-background/50"
                )}>
                  <pre className="whitespace-pre-wrap break-all">{message.result}</pre>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function ToolInput({ toolName, input }: { toolName: string; input: Record<string, unknown> }) {
  const getString = (key: string): string => String(input[key] || "");
  const getNumber = (key: string): number | undefined => {
    const val = input[key];
    return typeof val === "number" ? val : undefined;
  };

  switch (toolName) {
    case "Bash":
      return (
        <div className="font-mono text-sm">
          <span className="text-muted-foreground">$ </span>
          <span className="text-foreground">{getString("command")}</span>
          {getString("description") && (
            <p className="text-xs text-muted-foreground mt-1">{getString("description")}</p>
          )}
        </div>
      );

    case "Read": {
      const offset = getNumber("offset");
      return (
        <div className="font-mono text-sm text-foreground">
          {getString("file_path")}
          {offset !== undefined && <span className="text-muted-foreground"> (line {offset})</span>}
        </div>
      );
    }

    case "Write": {
      const content = getString("content");
      return (
        <div>
          <div className="font-mono text-sm text-foreground mb-2">{getString("file_path")}</div>
          {content && (
            <pre className="text-xs bg-background/50 p-2 rounded overflow-x-auto max-h-32 overflow-y-auto">
              {content.slice(0, 500)}
              {content.length > 500 && "..."}
            </pre>
          )}
        </div>
      );
    }

    case "Edit": {
      const oldString = getString("old_string");
      const newString = getString("new_string");
      return (
        <div>
          <div className="font-mono text-sm text-foreground mb-2">{getString("file_path")}</div>
          {oldString && (
            <div className="text-xs mb-1">
              <span className="text-destructive">- </span>
              <span className="font-mono bg-destructive/10 px-1">{oldString.slice(0, 100)}</span>
            </div>
          )}
          {newString && (
            <div className="text-xs">
              <span className="text-green-600">+ </span>
              <span className="font-mono bg-green-600/10 px-1">{newString.slice(0, 100)}</span>
            </div>
          )}
        </div>
      );
    }

    case "Glob":
      return (
        <div className="font-mono text-sm text-foreground">
          {getString("pattern")}
          {getString("path") && <span className="text-muted-foreground"> in {getString("path")}</span>}
        </div>
      );

    case "Grep":
      return (
        <div className="font-mono text-sm text-foreground">
          /{getString("pattern")}/
          {getString("path") && <span className="text-muted-foreground"> in {getString("path")}</span>}
        </div>
      );

    case "WebFetch":
    case "WebSearch":
      return (
        <div className="font-mono text-sm text-foreground break-all">
          {getString("url") || getString("query")}
        </div>
      );

    default:
      return (
        <pre className="text-xs font-mono overflow-x-auto">
          {JSON.stringify(input, null, 2)}
        </pre>
      );
  }
}
