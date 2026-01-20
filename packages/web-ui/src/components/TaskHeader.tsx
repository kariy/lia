import { useState } from "react";
import type { TaskResponse } from "@lia/shared";
import { resumeTask, stopTask } from "../api";
import { useTaskStore } from "../store";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Copy } from "lucide-react";

interface TaskHeaderProps {
  task: TaskResponse;
}

export function TaskHeader({ task }: TaskHeaderProps) {
  const [loading, setLoading] = useState(false);
  const setTask = useTaskStore((state) => state.setTask);

  const handleResume = async () => {
    setLoading(true);
    try {
      const updated = await resumeTask(task.id);
      setTask(updated);
    } catch (error) {
      console.error("Failed to resume task:", error);
    } finally {
      setLoading(false);
    }
  };

  const handleStop = async () => {
    if (!confirm("Are you sure you want to end this session? All resources will be released.")) {
      return;
    }

    setLoading(true);
    try {
      await stopTask(task.id);
      setTask({ ...task, status: "terminated" });
    } catch (error) {
      console.error("Failed to stop task:", error);
    } finally {
      setLoading(false);
    }
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text).catch(console.error);
  };

  return (
    <div>
      <header className="flex items-center justify-between border-b bg-background px-4 py-3">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold text-foreground">Lia</h1>
          <div className="flex items-center gap-2">
            <StatusBadge status={task.status} />
            <span className="text-sm text-muted-foreground font-mono">
              {task.id}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2">

          {task.status === "suspended" && (
            <Button
              onClick={handleResume}
              disabled={loading}
              size="sm"
            >
              {loading ? "Resuming..." : "Resume"}
            </Button>
          )}

          {(task.status === "running" || task.status === "suspended") && (
            <Button
              onClick={handleStop}
              disabled={loading}
              variant="outline"
              size="sm"
            >
              {loading ? "Stopping..." : "End Session"}
            </Button>
          )}
        </div>
      </header>

      {/* SSH Connection Info */}
      {task.status === "running" && task.ssh_command && (
        <div className="flex items-center justify-between border-b bg-secondary/50 px-4 py-2">
          <div className="flex items-center gap-3">
            <span className="text-xs font-medium text-muted-foreground">SSH:</span>
            <code className="rounded border bg-background px-2 py-1 text-xs font-mono text-foreground">
              {task.ssh_command}
            </code>
            <Button
              onClick={() => copyToClipboard(task.ssh_command!)}
              variant="ghost"
              size="sm"
              className="h-7 px-2"
            >
              <Copy className="h-3 w-3 mr-1" />
              Copy
            </Button>
          </div>
          <span className="text-xs text-muted-foreground">
            Use VS Code Remote SSH or your terminal to connect
          </span>
        </div>
      )}
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const config: Record<string, { variant: "running" | "pending" | "suspended" | "terminated" | "secondary"; label: string }> = {
    pending: { variant: "pending", label: "Pending" },
    starting: { variant: "pending", label: "Starting" },
    running: { variant: "running", label: "Running" },
    suspended: { variant: "suspended", label: "Suspended" },
    terminated: { variant: "terminated", label: "Terminated" },
  };

  const { variant, label } = config[status] || { variant: "secondary" as const, label: status };

  return (
    <Badge variant={variant}>
      <span className="mr-1 h-1.5 w-1.5 animate-pulse rounded-full bg-current"></span>
      {label}
    </Badge>
  );
}
