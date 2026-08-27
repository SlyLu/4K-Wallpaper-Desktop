ALTER TABLE monitor_schedule
ADD COLUMN selection_mode TEXT NOT NULL DEFAULT 'round_robin'
CHECK (selection_mode IN ('round_robin', 'random'));
