-- Add source and repositories columns to tasks table

ALTER TABLE tasks ADD COLUMN source VARCHAR(16) NOT NULL DEFAULT 'web';
ALTER TABLE tasks ADD COLUMN repositories TEXT[] NOT NULL DEFAULT '{}';

-- Add index on source for filtering
CREATE INDEX idx_tasks_source ON tasks (source);
