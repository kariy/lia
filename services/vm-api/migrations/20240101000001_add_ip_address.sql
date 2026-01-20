-- Add IP address column to tasks table
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS ip_address VARCHAR(15);

-- Create index for IP lookup
CREATE INDEX IF NOT EXISTS idx_tasks_ip_address ON tasks(ip_address);
