import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { createTask } from "../api";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface NewAgentPageProps {
  onTaskCreated: () => void;
  onCancel: () => void;
}

export function NewAgentPage({ onTaskCreated, onCancel }: NewAgentPageProps) {
  const navigate = useNavigate();
  const [prompt, setPrompt] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!prompt.trim() || isSubmitting) return;

    setIsSubmitting(true);
    setError(null);

    try {
      const task = await createTask(prompt.trim());
      onTaskCreated();
      navigate(`/tasks/${task.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create task");
      setIsSubmitting(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      onCancel();
    }
  }

  return (
    <div className="flex h-full items-center justify-center bg-background p-6">
      <div className="w-full max-w-xl">
        <form onSubmit={handleSubmit} onKeyDown={handleKeyDown}>
          <h2 className="text-lg font-semibold text-foreground mb-4">
            New Agent
          </h2>
          <Textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="What would you like the agent to do?"
            className="min-h-[160px] text-base mb-4"
            disabled={isSubmitting}
            autoFocus
          />
          {error && (
            <p className="text-sm text-destructive mb-4">{error}</p>
          )}
          <div className="flex gap-3">
            <Button
              type="submit"
              disabled={!prompt.trim() || isSubmitting}
            >
              {isSubmitting ? "Creating..." : "Create Agent"}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              disabled={isSubmitting}
            >
              Cancel
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}
