-- Initialize Ra planner extension for PostgreSQL

-- Create extension
CREATE EXTENSION IF NOT EXISTS pg_ra_planner;

-- Configure Ra optimizer settings
ALTER SYSTEM SET ra_planner.enabled = on;
ALTER SYSTEM SET ra_planner.log_plans = on;

-- Reload configuration
SELECT pg_reload_conf();

-- Log successful initialization
DO $$
BEGIN
  RAISE NOTICE 'Ra planner extension initialized successfully';
END $$;
