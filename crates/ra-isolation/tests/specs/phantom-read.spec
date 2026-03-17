# Phantom Read Test
#
# Tests whether a transaction sees rows inserted by another
# committed transaction when re-executing the same query.
# Under REPEATABLE READ or SERIALIZABLE, the second read
# in s1 should return the same result as the first.

setup
{
    CREATE TABLE items (id INT PRIMARY KEY, category TEXT);
    INSERT INTO items VALUES (1, 'A');
    INSERT INTO items VALUES (2, 'A');
    INSERT INTO items VALUES (3, 'B');
}

teardown
{
    DROP TABLE items;
}

session "s1"
{
    step "first_read"
    {
        BEGIN;
        SELECT COUNT(*) FROM items WHERE category = 'A';
        -- @marker s1_read_done
    }

    step "second_read"
    {
        -- @wait s2_committed
        SELECT COUNT(*) FROM items WHERE category = 'A';
        COMMIT;
    }
}

session "s2"
{
    step "insert_and_commit"
    {
        -- @wait s1_read_done
        BEGIN;
        INSERT INTO items VALUES (4, 'A');
        COMMIT;
        -- @marker s2_committed
    }
}

# Under READ COMMITTED:
#   s1:first_read  -> count=2
#   s2:insert       -> inserts id=4
#   s1:second_read -> count=3 (phantom read occurs)
#
# Under REPEATABLE READ / SERIALIZABLE:
#   s1:first_read  -> count=2
#   s2:insert       -> inserts id=4
#   s1:second_read -> count=2 (no phantom read)

permutation
{
    s1:first_read
    s2:insert_and_commit
    s1:second_read
}
