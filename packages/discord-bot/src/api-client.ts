import type {
  CreateTaskRequest,
  TaskResponse,
  TaskListResponse,
} from "@lia/shared";
import { config } from "./config";

export class VmApiClient {
  private baseUrl: string;

  constructor() {
    this.baseUrl = config.vmApiUrl;
  }

  async createTask(request: CreateTaskRequest): Promise<TaskResponse> {
    const response = await fetch(`${this.baseUrl}/api/v1/tasks`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(request),
    });

    if (!response.ok) {
      const error = await response.json().catch(() => ({}));
      throw new Error(error.error || `API error: ${response.status}`);
    }

    return response.json();
  }

  async getTask(taskId: string): Promise<TaskResponse> {
    const response = await fetch(`${this.baseUrl}/api/v1/tasks/${taskId}`);

    if (!response.ok) {
      if (response.status === 404) {
        throw new Error("Task not found");
      }
      const error = await response.json().catch(() => ({}));
      throw new Error(error.error || `API error: ${response.status}`);
    }

    return response.json();
  }

  async listTasks(
    userId?: string,
    status?: string,
    page = 1,
    perPage = 10
  ): Promise<TaskListResponse> {
    const params = new URLSearchParams();
    if (userId) params.set("user_id", userId);
    if (status) params.set("status", status);
    params.set("page", String(page));
    params.set("per_page", String(perPage));

    const response = await fetch(
      `${this.baseUrl}/api/v1/tasks?${params.toString()}`
    );

    if (!response.ok) {
      const error = await response.json().catch(() => ({}));
      throw new Error(error.error || `API error: ${response.status}`);
    }

    return response.json();
  }

  async resumeTask(taskId: string): Promise<TaskResponse> {
    const response = await fetch(
      `${this.baseUrl}/api/v1/tasks/${taskId}/resume`,
      {
        method: "POST",
      }
    );

    if (!response.ok) {
      const error = await response.json().catch(() => ({}));
      throw new Error(error.error || `API error: ${response.status}`);
    }

    return response.json();
  }

  async deleteTask(taskId: string): Promise<void> {
    const response = await fetch(`${this.baseUrl}/api/v1/tasks/${taskId}`, {
      method: "DELETE",
    });

    if (!response.ok) {
      const error = await response.json().catch(() => ({}));
      throw new Error(error.error || `API error: ${response.status}`);
    }
  }
}

export const apiClient = new VmApiClient();
