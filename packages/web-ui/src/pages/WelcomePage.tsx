export function WelcomePage() {
  return (
    <div className="flex h-full items-center justify-center bg-background">
      <div className="text-center max-w-md px-6">
        <h1 className="text-2xl font-semibold text-foreground mb-2">
          Welcome to Lia
        </h1>
        <p className="text-muted-foreground">
          Select a task from the sidebar or create a new agent to get started.
        </p>
      </div>
    </div>
  );
}
