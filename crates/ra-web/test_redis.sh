#!/usr/bin/env bash
set -euo pipefail

# Test Redis connectivity
echo "Testing Redis connection..."
redis-cli -h redis ping 2>&1 || {
    echo "Redis not available at 'redis' host"
    redis-cli -h localhost ping 2>&1 || {
        echo "Redis not available at localhost either"
        exit 1
    }
}

echo "Redis is available!"

# Test setting and getting a value
redis-cli -h redis SET test:key "test value" >/dev/null 2>&1 || redis-cli SET test:key "test value"
redis-cli -h redis GET test:key 2>&1 || redis-cli GET test:key
redis-cli -h redis DEL test:key >/dev/null 2>&1 || redis-cli DEL test:key >/dev/null

echo "Redis read/write test passed!"
