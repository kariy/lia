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
    connectionStatus,
    error,
    bootStage,
    bootMessage,
    setTask,
    setConnectionStatus,
    setError,
    processOutput,
    setWebSocket,
    setBootProgress,
    reset,
  } = useTaskStore();

  useEffect(() => {
    if (!taskId) {
      navigate("/");
      return;
    }

    reset();
    setConnectionStatus("loading");

    getTask(taskId)
      .then((fetchedTask) => {
        setTask(fetchedTask);

        // Connect WebSocket after task is loaded
        const ws = createWebSocket(taskId);
        wsRef.current = ws;

        ws.onopen = () => {
          setConnectionStatus("connected");
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
              case "progress":
                // Update boot progress
                store.setBootProgress(msg.stage, msg.message);
                // If task is still starting and we got progress, update status
                if (store.task && store.task.status === "starting") {
                  // When ready, update task status to running
                  if (msg.stage === "ready") {
                    store.setTask({ ...store.task, status: "running" });
                  }
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
          setConnectionStatus("error");
        };

        ws.onclose = () => {
          setConnectionStatus("idle");
        };

        setWebSocket(ws);
      })
      .catch((err) => {
        setError(err.message);
        setConnectionStatus("error");
      });

    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [taskId, navigate, reset, setConnectionStatus, setTask, setError, processOutput, setWebSocket, setBootProgress]);

  if (connectionStatus === "loading") {
    return (
      <div className="flex h-full items-center justify-center bg-background">
        <div className="text-center">
          <Loader2 className="mb-4 h-8 w-8 animate-spin text-foreground mx-auto" />
          <p className="text-muted-foreground">Loading task...</p>
        </div>
      </div>
    );
  }

  if (connectionStatus === "error" || !task) {
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

  // Show boot progress when task is starting
  if (task.status === "starting" || task.status === "pending") {
    return (
      <div className="flex h-full flex-col bg-background">
        <TaskHeader task={task} />
        <div className="flex flex-1 items-center justify-center">
          <div className="text-center">
            <Loader2 className="mb-4 h-8 w-8 animate-spin text-foreground mx-auto" />
            <p className="text-foreground font-medium">
              {bootMessage || "Starting VM..."}
            </p>
            {bootStage && bootStage !== "ready" && (
              <p className="text-muted-foreground text-sm mt-2">
                This may take a few moments
              </p>
            )}
          </div>
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
