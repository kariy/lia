import { create } from "zustand";
import type { TaskResponse, WsInputMessage } from "@lia/shared";
import type { ParsedMessage } from "./types/claude-stream";
import { MessageParser } from "./lib/message-parser";

interface TaskState {
  task: TaskResponse | null;
  status: "idle" | "loading" | "error" | "connected";
  error: string | null;
  messages: ParsedMessage[];
  ws: WebSocket | null;
  parser: MessageParser;

  setTask: (task: TaskResponse) => void;
  setStatus: (status: TaskState["status"]) => void;
  setError: (error: string | null) => void;
  processOutput: (data: string) => void;
  setWebSocket: (ws: WebSocket | null) => void;
  sendInput: (data: string) => void;
  reset: () => void;
}

export const useTaskStore = create<TaskState>((set, get) => ({
  task: null,
  status: "idle",
  error: null,
  messages: [],
  ws: null,
  parser: new MessageParser(),

  setTask: (task) => set({ task }),
  setStatus: (status) => set({ status }),
  setError: (error) => set({ error }),

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
      status: "idle",
      error: null,
      messages: [],
      ws: null,
      parser: newParser,
    });
  },
}));
