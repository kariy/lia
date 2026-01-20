import { useState, useEffect } from "react";
import { Link, useParams, Outlet } from "react-router-dom";
import { listTasks } from "../api";
import type { TaskResponse } from "@lia/shared";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Plus, ChevronLeft, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { NewAgentPage } from "../pages/NewAgentPage";

export function Layout() {
  const { taskId } = useParams();
  const [tasks, setTasks] = useState<TaskResponse[]>([]);
  const [isLoadingTasks, setIsLoadingTasks] = useState(true);
  const [isCollapsed, setIsCollapsed] = useState(false);
  const [showNewAgent, setShowNewAgent] = useState(false);

  useEffect(() => {
    loadTasks();
  }, []);

  // Close new agent form when navigating to a task
  useEffect(() => {
    if (taskId) {
      setShowNewAgent(false);
    }
  }, [taskId]);

  async function loadTasks() {
    try {
      const response = await listTasks();
      setTasks(response.tasks);
    } catch (err) {
      console.error("Failed to load tasks:", err);
    } finally {
      setIsLoadingTasks(false);
    }
  }

  function handleTaskCreated() {
    loadTasks();
    setShowNewAgent(false);
  }

  function handleCancel() {
    setShowNewAgent(false);
  }

  function getStatusVariant(status: string) {
    switch (status) {
      case "running":
        return "running";
      case "pending":
      case "starting":
        return "pending";
      case "suspended":
        return "suspended";
      case "terminated":
        return "terminated";
      default:
        return "secondary";
    }
  }

  function formatDate(dateString: string) {
    const date = new Date(dateString);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const days = Math.floor(diff / (1000 * 60 * 60 * 24));

    if (days === 0) {
      return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    } else if (days === 1) {
      return 'Yesterday';
    } else if (days < 7) {
      return `${days} days ago`;
    }
    return date.toLocaleDateString();
  }

  return (
    <div className="flex h-screen bg-background">
      {/* Sidebar */}
      <aside
        className={cn(
          "flex flex-col border-r bg-secondary/30 transition-all duration-200",
          isCollapsed ? "w-12" : "w-80"
        )}
      >
        {/* Sidebar Header */}
        <div className="flex items-center justify-between border-b px-3 py-3">
          {!isCollapsed && (
            <Link to="/" className="flex items-center gap-2" onClick={() => setShowNewAgent(false)}>
              <span className="text-lg font-semibold text-foreground">Lia</span>
            </Link>
          )}
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={() => setIsCollapsed(!isCollapsed)}
          >
            {isCollapsed ? (
              <ChevronRight className="h-4 w-4" />
            ) : (
              <ChevronLeft className="h-4 w-4" />
            )}
          </Button>
        </div>

        {!isCollapsed && (
          <>
            {/* New Task Button */}
            <div className="border-b p-3">
              <Button
                onClick={() => setShowNewAgent(true)}
                className="w-full"
                size="sm"
                variant={showNewAgent ? "secondary" : "default"}
              >
                <Plus className="h-4 w-4 mr-2" />
                New Agent
              </Button>
            </div>

            {/* Task List */}
            <div className="flex-1 overflow-y-auto">
              {isLoadingTasks ? (
                <div className="p-3 text-sm text-muted-foreground">
                  Loading...
                </div>
              ) : tasks.length === 0 ? (
                <div className="p-3 text-sm text-muted-foreground">
                  No tasks yet
                </div>
              ) : (
                <div className="py-1">
                  {tasks.map((task) => (
                    <Link
                      key={task.id}
                      to={`/tasks/${task.id}`}
                      onClick={() => setShowNewAgent(false)}
                      className={cn(
                        "flex flex-col gap-1 px-3 py-2 transition-colors hover:bg-accent",
                        taskId === task.id && !showNewAgent && "bg-accent"
                      )}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="truncate text-sm font-mono text-foreground">
                          {task.id.slice(0, 8)}
                        </span>
                        <Badge
                          variant={getStatusVariant(task.status) as "running" | "pending" | "suspended" | "terminated" | "secondary"}
                          className="shrink-0 text-[10px] px-1.5 py-0"
                        >
                          {task.status}
                        </Badge>
                      </div>
                      <span className="text-xs text-muted-foreground">
                        {formatDate(task.created_at)}
                      </span>
                    </Link>
                  ))}
                </div>
              )}
            </div>
          </>
        )}
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-hidden">
        {showNewAgent ? (
          <NewAgentPage onTaskCreated={handleTaskCreated} onCancel={handleCancel} />
        ) : (
          <Outlet />
        )}
      </main>
    </div>
  );
}
