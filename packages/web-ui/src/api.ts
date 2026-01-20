import type { CreateTaskRequest, TaskResponse, TaskListResponse } from "@lia/shared";

const API_BASE = "/api/v1";

// Generate a UUID using crypto.getRandomValues (broader browser support)
function generateUUID(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  // Set version (4) and variant (RFC4122)
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

// Generate or retrieve a persistent user ID for web users
function getWebUserId(): string {
  const storageKey = "lia-web-user-id";
  let userId = localStorage.getItem(storageKey);
  if (!userId) {
    userId = `web-${generateUUID()}`;
    localStorage.setItem(storageKey, userId);
  }
  return userId;
}

export async function createTask(
  prompt: string,
  repositories: string[],
  options?: {
    config?: CreateTaskRequest["config"];
    files?: CreateTaskRequest["files"];
    ssh_public_key?: string;
  }
): Promise<TaskResponse> {
  const request: CreateTaskRequest = {
    prompt,
    repositories,
    source: "web",
    user_id: getWebUserId(),
    ...options,
  };

  const response = await fetch(`${API_BASE}/tasks`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    throw new Error(error.error || `API error: ${response.status}`);
  }

  return response.json();
}

export async function listTasks(): Promise<TaskListResponse> {
  const response = await fetch(`${API_BASE}/tasks?user_id=${encodeURIComponent(getWebUserId())}`);

  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    throw new Error(error.error || `API error: ${response.status}`);
  }

  return response.json();
}

export async function getTask(taskId: string): Promise<TaskResponse> {
  const response = await fetch(`${API_BASE}/tasks/${taskId}`);

  if (!response.ok) {
    if (response.status === 404) {
      throw new Error("Task not found");
    }
    const error = await response.json().catch(() => ({}));
    throw new Error(error.error || `API error: ${response.status}`);
  }

  return response.json();
}

export async function resumeTask(taskId: string): Promise<TaskResponse> {
  const response = await fetch(`${API_BASE}/tasks/${taskId}/resume`, {
    method: "POST",
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    throw new Error(error.error || `API error: ${response.status}`);
  }

  return response.json();
}

export async function stopTask(taskId: string): Promise<void> {
  const response = await fetch(`${API_BASE}/tasks/${taskId}`, {
    method: "DELETE",
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    throw new Error(error.error || `API error: ${response.status}`);
  }
}

export function createWebSocket(taskId: string): WebSocket {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const host = window.location.host;
  return new WebSocket(`${protocol}//${host}${API_BASE}/tasks/${taskId}/stream`);
}
