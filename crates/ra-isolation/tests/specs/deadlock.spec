# Deadlock Test
#
# Two sessions each hold a lock on one row and try to acquire
# a lock on the row held by the other, creating a deadlock.
# The database should detect this and abort one transaction.

setup
{
    CREATE TABLE resources (id INT PRIMARY KEY, value INT);
    INSERT INTO resources VALUES (1, 100);
    INSERT INTO resources VALUES (2, 200);
}

teardown
{
    DROP TABLE resources;
}

session "s1"
{
    step "lock_r1"
    {
        BEGIN;
        UPDATE resources SET value = value + 10 WHERE id = 1;
        -- @marker s1_locked_r1
    }

    step "try_lock_r2"
    {
        -- @wait s2_locked_r2
        UPDATE resources SET value = value + 10 WHERE id = 2;
        COMMIT;
    }
}

session "s2"
{
    step "lock_r2"
    {
        BEGIN;
        UPDATE resources SET value = value + 20 WHERE id = 2;
        -- @marker s2_locked_r2
    }

    step "try_lock_r1"
    {
        -- @wait s1_locked_r1
        UPDATE resources SET value = value + 20 WHERE id = 1;
        COMMIT;
    }
}

# This permutation creates a deadlock:
# s1 locks r1, s2 locks r2, then each tries to lock
# the other's resource. The database detects the cycle
# and aborts one transaction with a deadlock error.

permutation
{
    s1:lock_r1
    s2:lock_r2
    s1:try_lock_r2
    s2:try_lock_r1
}
