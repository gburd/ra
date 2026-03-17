# Dirty Read Test
#
# Tests whether a transaction can see uncommitted data from
# another transaction. Under READ COMMITTED or higher isolation,
# session s2 should NOT see s1's uncommitted write.

setup
{
    CREATE TABLE accounts (id INT PRIMARY KEY, balance INT);
    INSERT INTO accounts VALUES (1, 1000);
}

teardown
{
    DROP TABLE accounts;
}

session "s1"
{
    step "begin_and_write"
    {
        BEGIN;
        UPDATE accounts SET balance = 500 WHERE id = 1;
        -- @marker s1_wrote
    }

    step "rollback"
    {
        ROLLBACK;
    }
}

session "s2"
{
    step "read_during_s1"
    {
        -- @wait s1_wrote
        BEGIN;
        SELECT balance FROM accounts WHERE id = 1;
        COMMIT;
    }
}

# Under READ COMMITTED, s2 reads balance=1000 (committed value).
# Under READ UNCOMMITTED, s2 might read balance=500 (dirty read).

permutation
{
    s1:begin_and_write
    s2:read_during_s1
    s1:rollback
}
