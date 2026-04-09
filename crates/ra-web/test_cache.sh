#!/usr/bin/env bash
set -euo pipefail

# Test Redis caching for EXPLAIN endpoint
# This script tests that the cache is working correctly by:
# 1. Making an EXPLAIN request and timing it
# 2. Making the same request again and verifying it's faster (cache hit)
# 3. Making a different request and verifying it's not cached

BASE_URL="${BASE_URL:-http://localhost:8000}"

echo "Testing Redis cache for EXPLAIN endpoint..."
echo "Base URL: $BASE_URL"

# Test 1: First request (cache miss)
echo ""
echo "Test 1: First request (cache miss)"
START=$(date +%s%N)
RESPONSE1=$(curl -s -X POST "$BASE_URL/api/explain" \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM users WHERE age > 25","engine":"sqlite","analyze":false}')
END=$(date +%s%N)
TIME1=$((($END - $START) / 1000000))
echo "Response 1: $RESPONSE1"
echo "Time 1: ${TIME1}ms"

# Extract execution time from response
EXEC_TIME1=$(echo "$RESPONSE1" | jq -r '.execution_time_ms')
echo "Execution time 1: ${EXEC_TIME1}ms"

# Test 2: Second request (should be cache hit)
echo ""
echo "Test 2: Second request (cache hit)"
sleep 1
START=$(date +%s%N)
RESPONSE2=$(curl -s -X POST "$BASE_URL/api/explain" \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM users WHERE age > 25","engine":"sqlite","analyze":false}')
END=$(date +%s%N)
TIME2=$((($END - $START) / 1000000))
echo "Response 2: $RESPONSE2"
echo "Time 2: ${TIME2}ms"

# Extract execution time from response (should be 0 for cache hit)
EXEC_TIME2=$(echo "$RESPONSE2" | jq -r '.execution_time_ms')
echo "Execution time 2: ${EXEC_TIME2}ms"

# Verify cache hit (execution_time_ms should be 0)
if [ "$(echo "$EXEC_TIME2 == 0" | bc -l)" -eq 1 ]; then
    echo "✓ Cache hit verified (execution_time_ms = 0)"
else
    echo "✗ Cache miss detected (execution_time_ms = $EXEC_TIME2)"
    exit 1
fi

# Test 3: Different request (cache miss)
echo ""
echo "Test 3: Different request (cache miss)"
START=$(date +%s%N)
RESPONSE3=$(curl -s -X POST "$BASE_URL/api/explain" \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT COUNT(*) FROM orders","engine":"sqlite","analyze":false}')
END=$(date +%s%N)
TIME3=$((($END - $START) / 1000000))
echo "Response 3: $RESPONSE3"
echo "Time 3: ${TIME3}ms"

EXEC_TIME3=$(echo "$RESPONSE3" | jq -r '.execution_time_ms')
echo "Execution time 3: ${EXEC_TIME3}ms"

# Verify different query is not cached (execution_time_ms should be > 0)
if [ "$(echo "$EXEC_TIME3 > 0" | bc -l)" -eq 1 ]; then
    echo "✓ Cache miss verified for different query (execution_time_ms > 0)"
else
    echo "✗ Unexpected cache hit for different query"
    exit 1
fi

# Test 4: Same query with analyze=true (different cache key)
echo ""
echo "Test 4: Same query with analyze=true (cache miss)"
START=$(date +%s%N)
RESPONSE4=$(curl -s -X POST "$BASE_URL/api/explain" \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM users WHERE age > 25","engine":"sqlite","analyze":true}')
END=$(date +%s%N)
TIME4=$((($END - $START) / 1000000))
echo "Response 4: $RESPONSE4"
echo "Time 4: ${TIME4}ms"

EXEC_TIME4=$(echo "$RESPONSE4" | jq -r '.execution_time_ms')
echo "Execution time 4: ${EXEC_TIME4}ms"

# Verify analyze flag creates different cache key (execution_time_ms should be > 0)
if [ "$(echo "$EXEC_TIME4 > 0" | bc -l)" -eq 1 ]; then
    echo "✓ Different cache key verified for analyze=true (execution_time_ms > 0)"
else
    echo "✗ Unexpected cache hit with different analyze flag"
    exit 1
fi

echo ""
echo "✓ All cache tests passed!"
