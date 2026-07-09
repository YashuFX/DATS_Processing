#!/usr/bin/env bash
# ── Integration Smoke Test ────────────────────────────────────────────────────
#
# Responsibility: Runs the full composition pipeline against a local RabbitMQ,
# publishes a test packet, consumes the identified packet from telemetry.identified,
# and verifies all mutated and structured fields.

set -eo pipefail

echo "======================================================================"
echo "Starting Mission Identification Service Integration Smoke Test"
echo "======================================================================"

# 1. Config environment
export PROTOC="/home/admin-yash/Desktop/Decode/bin/bin/protoc"
export AMQP_URL="amqp://guest:guest@localhost:5672/%2f"
export SOURCE_EXCHANGE="telemetry.decoded"
export SOURCE_QUEUE="mission.identify"
export SOURCE_ROUTING_KEY="#.decoded"
export DESTINATION_EXCHANGE="telemetry.identified"
export METRICS_PORT="8083"

# Create clean log files
MIS_LOG="/tmp/mis_smoke_test.log"
CONSUMER_LOG="/tmp/mis_consumer_smoke_test.log"
rm -f "$MIS_LOG" "$CONSUMER_LOG"
touch "$MIS_LOG" "$CONSUMER_LOG"

# 2. Build the binaries first to avoid build time in running time
echo "Building binaries..."
cargo build --bin mission-identification-service --bin publish-decoded-envelope --bin consume-identified-envelope

# 3. Start the test consumer in the background
echo "Starting identified envelope test consumer..."
cargo run --bin consume-identified-envelope > "$CONSUMER_LOG" 2>&1 &
CONSUMER_PID=$!

# 4. Start the service in the background
echo "Starting Mission Identification Service..."
cargo run --bin mission-identification-service > "$MIS_LOG" 2>&1 &
MIS_PID=$!

# Cleanup trap to ensure background processes are always terminated
cleanup() {
  echo "Cleaning up processes..."
  if kill -0 $MIS_PID 2>/dev/null; then
    kill $MIS_PID
    wait $MIS_PID 2>/dev/null || true
  fi
  if kill -0 $CONSUMER_PID 2>/dev/null; then
    kill $CONSUMER_PID
    wait $CONSUMER_PID 2>/dev/null || true
  fi
}
trap cleanup EXIT

# Wait for service & consumer to start up and connect to RabbitMQ
echo "Waiting for services to establish AMQP connections..."
sleep 4

# 5. Run the publish helper to send the packet
echo "Publishing test telemetry envelope..."
cargo run --bin publish-decoded-envelope

# Wait for consumption, processing, publishing and receipt
echo "Waiting for message pipeline processing..."
sleep 4

# 6. Stop the background processes
cleanup
trap - EXIT

echo "======================================================================"
echo "Verifying processed output in logs"
echo "======================================================================"

echo "--- SERVICE LOGS ---"
cat "$MIS_LOG"
echo ""
echo "--- TEST CONSUMER LOGS ---"
cat "$CONSUMER_LOG"
echo ""

# Execute assertions
FAILED=0

check_file_log() {
  local log_file="$1"
  local pattern="$2"
  local desc="$3"
  if grep -q "$pattern" "$log_file"; then
    echo "  [PASS] $desc"
  else
    echo "  [FAIL] $desc (pattern '$pattern' not found in $log_file)"
    FAILED=1
  fi
}

echo "=== Running Ingress & Core Assertions ==="
check_file_log "$MIS_LOG" "Mission Identification Service — Starting..." "Service started successfully"
check_file_log "$MIS_LOG" "Registry loaded successfully" "Registry configured and loaded"

echo "=== Running Egress & Mutation Assertions ==="
check_file_log "$CONSUMER_LOG" "\[ASSERT_STAGE=6\]" "Verified stage mutated to Identified (6)"
check_file_log "$CONSUMER_LOG" "\[ASSERT_MISSION_CODE=cy3\]" "Verified mission code resolved to cy3"
check_file_log "$CONSUMER_LOG" "\[ASSERT_SAT_ID=101\]" "Verified satellite ID resolved to 101"
check_file_log "$CONSUMER_LOG" "\[ASSERT_ROUTING_KEY=cy3.sat101.42.identified\]" "Verified routing key resolved to cy3.sat101.42.identified"

if [ $FAILED -eq 0 ]; then
  echo "======================================================================"
  echo "INTEGRATION SMOKE TEST PASSED"
  echo "======================================================================"
  exit 0
else
  echo "======================================================================"
  echo "INTEGRATION SMOKE TEST FAILED"
  echo "======================================================================"
  exit 1
fi
