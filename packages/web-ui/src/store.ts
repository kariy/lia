import { create } from "zustand";
import type { TaskResponse, WsInputMessage, BootStage } from "@lia/shared";
import type { ParsedMessage } from "./types/claude-stream";
import { MessageParser } from "./lib/message-parser";

interface TaskState {
  task: TaskResponse | null;
  connectionStatus: "idle" | "loading" | "error" | "connected";
  error: string | null;
  messages: ParsedMessage[];
  ws: WebSocket | null;
  parser: MessageParser;
  isScrolledToBottom: boolean;
  bootStage: BootStage | null;
  bootMessage: string | null;

  setTask: (task: TaskResponse) => void;
  setConnectionStatus: (status: TaskState["connectionStatus"]) => void;
  setError: (error: string | null) => void;
  processOutput: (data: string) => void;
  setWebSocket: (ws: WebSocket | null) => void;
  sendInput: (data: string) => void;
  setIsScrolledToBottom: (value: boolean) => void;
  setBootProgress: (stage: BootStage, message: string) => void;
  reset: () => void;
}

export const useTaskStore = create<TaskState>((set, get) => ({
  task: null,
  connectionStatus: "idle",
  error: null,
  messages: [],
  ws: null,
  parser: new MessageParser(),
  isScrolledToBottom: true,
  bootStage: null,
  bootMessage: null,

  setTask: (task) => set({ task }),
  setConnectionStatus: (connectionStatus) => set({ connectionStatus }),
  setError: (error) => set({ error }),
  setIsScrolledToBottom: (value) => set({ isScrolledToBottom: value }),
  setBootProgress: (stage, message) => set({ bootStage: stage, bootMessage: message }),

  processOutput: (data) => {
    const { parser } = get();
    // Parse the JSON line and update messages
    parser.parseMessage(data);
    // Update messages array (creates new reference for React)
    set({ messages: parser.getMessages() });
  },

  setWebSocket: (ws) => set({ ws }),

  sendInput: (data) => {
    const { ws, parser } = get();
    if (ws && ws.readyState === WebSocket.OPEN) {
      const msg: WsInputMessage = { type: "input", data };
      ws.send(JSON.stringify(msg));
      // Add user input to messages
      parser.addUserInput(data.trim());
      set({ messages: parser.getMessages() });
    }
  },

  reset: () => {
    const newParser = new MessageParser();
    set({
      task: null,
      connectionStatus: "idle",
      error: null,
      messages: [],
      ws: null,
      parser: newParser,
      isScrolledToBottom: true,
      bootStage: null,
      bootMessage: null,
    });
  },
}));
