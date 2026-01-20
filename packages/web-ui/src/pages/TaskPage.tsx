import { useEffect, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { MessageList } from "../components/messages";
import { TaskHeader } from "../components/TaskHeader";
import { InputBar } from "../components/InputBar";
import { useTaskStore } from "../store";
import { getTask, createWebSocket } from "../api";
import type { WsMessage } from "@lia/shared";
import { Button } from "@/components/ui/button";
import { Loader2 } from "lucide-react";

export function TaskPage() {
  const { taskId } = useParams<{ taskId: string }>();
  const navigate = useNavigate();
  const wsRef = useRef<WebSocket | null>(null);

  const {
    task,
    status,
    error,
    setTask,
    setStatus,
    setError,
    processOutput,
    setWebSocket,
    reset,
  } = useTaskStore();

  useEffect(() => {
    if (!taskId) {
      navigate("/");
      return;
    }

    reset();
    setStatus("loading");

    getTask(taskId)
      .then((fetchedTask) => {
        setTask(fetchedTask);

        // Connect WebSocket after task is loaded
        const ws = createWebSocket(taskId);
        wsRef.current = ws;

        ws.onopen = () => {
          setStatus("connected");
        };

        ws.onmessage = (event) => {
          try {
            const msg: WsMessage = JSON.parse(event.data);
            const store = useTaskStore.getState();

            switch (msg.type) {
              case "output":
                // Process the output through the message parser
                store.processOutput(msg.data);
                break;
              case "status":
                if (store.task) {
                  store.setTask({ ...store.task, status: msg.status });
                }
                break;
              case "error":
                store.setError(msg.message);
                break;
            }
          } catch {
            console.error("Failed to parse WebSocket message");
          }
        };

        ws.onerror = () => {
          setError("WebSocket connection error");
          setStatus("error");
        };

        ws.onclose = () => {
          setStatus("idle");
        };

        setWebSocket(ws);
      })
      .catch((err) => {
        setError(err.message);
        setStatus("error");
      });

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [taskId, navigate, reset, setStatus, setTask, setError, processOutput, setWebSocket]);

  if (status === "loading") {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <div className="text-center">
          <Loader2 className="mb-4 h-8 w-8 animate-spin text-foreground mx-auto" />
          <p className="text-muted-foreground">Loading task...</p>
        </div>
      </div>
    );
  }

  if (status === "error" || !task) {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <div className="text-center">
          <p className="mb-4 text-destructive">{error || "Task not found"}</p>
          <Button onClick={() => navigate("/")}>
            Go Home
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-background">
      <TaskHeader task={task} />

      <div className="flex-1 min-h-0 overflow-hidden">
        <MessageList />
      </div>

      {(task.status === "running" || task.status === "suspended") && (
        <InputBar />
      )}
    </div>
  );
}
