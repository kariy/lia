import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { createTask } from "../api";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface NewAgentPageProps {
  onTaskCreated: () => void;
}

const REPO_REGEX = /^[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+$/;

export function NewAgentPage({ onTaskCreated }: NewAgentPageProps) {
  const navigate = useNavigate();
  const [repository, setRepository] = useState("");
  const [prompt, setPrompt] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isValidRepo = repository.trim() === "" || REPO_REGEX.test(repository.trim());

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!prompt.trim() || !repository.trim() || isSubmitting) return;

    if (!REPO_REGEX.test(repository.trim())) {
      setError("Invalid repository format. Use 'owner/repo' format.");
      return;
    }

    setIsSubmitting(true);
    setError(null);

    try {
      const task = await createTask(prompt.trim(), [repository.trim()]);
      onTaskCreated();
      navigate(`/tasks/${task.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create task");
      setIsSubmitting(false);
    }
  }

  return (
    <div className="flex h-full items-center justify-center bg-background p-6">
      <div className="w-full max-w-xl">
        <form onSubmit={handleSubmit}>
          <h2 className="text-lg font-semibold text-foreground mb-4">
            New Agent
          </h2>
          <div className="mb-4">
            <label
              htmlFor="repository"
              className="block text-sm font-medium text-foreground mb-2"
            >
              GitHub Repository
            </label>
            <input
              id="repository"
              type="text"
              value={repository}
              onChange={(e) => setRepository(e.target.value)}
              placeholder="owner/repo (e.g., facebook/react)"
              className={`flex h-10 w-full rounded-md border bg-background px-3 py-2 text-base ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 ${
                !isValidRepo ? "border-destructive" : "border-input"
              }`}
              disabled={isSubmitting}
              autoFocus
            />
            {!isValidRepo && (
              <p className="text-sm text-destructive mt-1">
                Invalid format. Use 'owner/repo' format.
              </p>
            )}
          </div>
          <div className="mb-4">
            <label
              htmlFor="prompt"
              className="block text-sm font-medium text-foreground mb-2"
            >
              Prompt
            </label>
            <Textarea
              id="prompt"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="What would you like the agent to do?"
              className="min-h-[160px] text-base"
              disabled={isSubmitting}
            />
          </div>
          {error && (
            <p className="text-sm text-destructive mb-4">{error}</p>
          )}
          <Button
            type="submit"
            disabled={!prompt.trim() || !repository.trim() || !isValidRepo || isSubmitting}
          >
            {isSubmitting ? "Creating..." : "Create Agent"}
          </Button>
        </form>
      </div>
    </div>
  );
}
