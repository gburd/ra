-- RA Planner Extension: PostgreSQL integration for the RA optimizer
--
-- Loaded via: CREATE EXTENSION ra_pg_extension;
--
-- GUC variables (configured via SET):
--   ra_planner.enabled         = on/off   (default: on)
--   ra_planner.min_confidence  = 0.0..1.0 (default: 0.9)
--   ra_planner.log_decisions   = on/off   (default: off)
--   ra_planner.max_relations   = 1..100   (default: 12)

-- The extension is loaded via shared_preload_libraries.
-- No SQL objects are created; all functionality operates through
-- planner hooks and GUC variables.

-- Verify the extension loaded correctly.
DO $$
BEGIN
    IF current_setting('ra_planner.enabled', true) IS NULL THEN
        RAISE WARNING 'ra_pg_extension: GUC variables not registered. '
            'Ensure the library is in shared_preload_libraries.';
    END IF;
END;
$$;
