#!/usr/bin/env bash
# ── Integration Smoke Test ────────────────────────────────────────────────────
#
# Responsibility: Runs the full composition pipeline against a local RabbitMQ,
# publishes a decommutated packet, consumes the converted packet, and verifies
# AST-based calculations and processing stage.

set -eo pipefail

echo "======================================================================"
echo "Starting Engineering Conversion Service Integration Smoke Test"
echo "======================================================================"

# 1. Config environment
export PROTOC="/home/admin-yash/Desktop/Decode/bin/bin/protoc"
export AMQP_URL="amqp://guest:guest@localhost:5672/%2f"
export SOURCE_EXCHANGE="telemetry.engineering"
export SOURCE_QUEUE="engineering.convert.smoke"
export SOURCE_ROUTING_KEY="#.decommutated"
export DESTINATION_EXCHANGE="telemetry.engineering"
export METRICS_PORT="8085"
export HEALTH_PORT="8086"
export DERIVED_DB_DIR="/tmp/ecs_derived_db"

# Create clean configuration directory and cy3.yaml file
rm -rf "$DERIVED_DB_DIR"
mkdir -p "$DERIVED_DB_DIR"

cat <<EOF > "$DERIVED_DB_DIR/cy3.yaml"
derived_parameters:
  - name: "/SC/BatteryPower"
    inputs:
      - parameter_name: "/SC/BatteryVoltage"
        alias: "v"
      - parameter_name: "/SC/BatteryCurrent"
        alias: "i"
    expression: "v * i"
    unit: "W"
EOF

# Create clean log files
ECS_LOG="/tmp/ecs_smoke_test.log"
CONSUMER_LOG="/tmp/ecs_consumer_smoke_test.log"
rm -f "$ECS_LOG" "$CONSUMER_LOG"
touch "$ECS_LOG" "$CONSUMER_LOG"

# 2. Build the binaries first to avoid build time in running time
echo "Building binaries..."
cargo build --bin engineering-conversion-service --bin publish-decommutated-envelope --bin consume-engineering-envelope

# 3. Start the test consumer in the background
echo "Starting engineering envelope test consumer..."
cargo run --bin consume-engineering-envelope > "$CONSUMER_LOG" 2>&1 &
CONSUMER_PID=$!

# 4. Start the service in the background
echo "Starting Engineering Conversion Service..."
cargo run --bin engineering-conversion-service > "$ECS_LOG" 2>&1 &
ECS_PID=$!

# Cleanup trap to ensure background processes are always terminated
cleanup() {
  echo "Cleaning up processes..."
  if kill -0 $ECS_PID 2>/dev/null; then
    kill $ECS_PID
    wait $ECS_PID 2>/dev/null || true
  fi
  if kill -0 $CONSUMER_PID 2>/dev/null; then
    kill $CONSUMER_PID
    wait $CONSUMER_PID 2>/dev/null || true
  fi
  rm -rf "$DERIVED_DB_DIR"
}
trap cleanup EXIT

# Wait for service & consumer to start up and connect to RabbitMQ
echo "Waiting for services to establish AMQP connections..."
sleep 4

# 5. Run the publish helper to send the packet
echo "Publishing test decommutated envelope..."
cargo run --bin publish-decommutated-envelope

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
cat "$ECS_LOG"
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
check_file_log "$ECS_LOG" "Engineering Conversion Service — Starting..." "Service started successfully"
check_file_log "$ECS_LOG" "Initializing formula registry" "Registry configured and loaded"
check_file_log "$ECS_LOG" "Health check server active" "Health check server activated"

echo "=== Running Egress & AST Calculation Assertions ==="
check_file_log "$CONSUMER_LOG" "\[ASSERT_STAGE=7\]" "Verified stage mutated to Engineering Converted (7)"
check_file_log "$CONSUMER_LOG" "\[ASSERT_ROUTING_KEY=cy3.sat101.42.engineering\]" "Verified outbound routing key cy3.sat101.42.engineering"
check_file_log "$CONSUMER_LOG" "\[ASSERT_BATTERY_POWER=98\]" "Verified calculated BatteryPower matches 28.0 * 3.5 = 98.0"

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
