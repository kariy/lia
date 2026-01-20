-- Create guild_tasks table to track task-guild relationships
CREATE TABLE IF NOT EXISTS guild_tasks (
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    guild_id VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (task_id)
);

-- Create indexes
CREATE INDEX idx_guild_tasks_guild_id ON guild_tasks(guild_id);
CREATE INDEX idx_guild_tasks_guild_created ON guild_tasks(guild_id, created_at DESC);
