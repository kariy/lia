import type { ParsedResult } from "../../types/claude-stream";
import { CheckCircle, XCircle, Clock, DollarSign } from "lucide-react";

interface ResultMessageProps {
  message: ParsedResult;
}

export function ResultMessage({ message }: ResultMessageProps) {
  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`;
    const seconds = ms / 1000;
    if (seconds < 60) return `${seconds.toFixed(1)}s`;
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = seconds % 60;
    return `${minutes}m ${remainingSeconds.toFixed(0)}s`;
  };

  const formatCost = (usd: number) => {
    if (usd < 0.01) return `$${usd.toFixed(4)}`;
    return `$${usd.toFixed(2)}`;
  };

  return (
    <div className="flex items-center justify-center gap-4 py-3 px-4 rounded-lg bg-secondary/30 text-sm">
      <div className="flex items-center gap-1.5">
        {message.success ? (
          <CheckCircle className="h-4 w-4 text-green-600" />
        ) : (
          <XCircle className="h-4 w-4 text-destructive" />
        )}
        <span className="text-muted-foreground">
          {message.success ? "Completed" : "Failed"}
        </span>
      </div>
      <div className="flex items-center gap-1.5 text-muted-foreground">
        <Clock className="h-4 w-4" />
        <span>{formatDuration(message.durationMs)}</span>
      </div>
      <div className="flex items-center gap-1.5 text-muted-foreground">
        <DollarSign className="h-4 w-4" />
        <span>{formatCost(message.costUsd)}</span>
      </div>
      <div className="text-muted-foreground">
        {message.numTurns} turn{message.numTurns !== 1 ? "s" : ""}
      </div>
    </div>
  );
}
