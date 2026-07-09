#!/usr/bin/env bash
# ── Integration Smoke Test ────────────────────────────────────────────────────
#
# Responsibility: Runs the full composition pipeline against a local RabbitMQ,
# publishes a test packet, consumes the decoded packet from telemetry.decoded,
# and verifies all mutated and structured fields.

set -eo pipefail

echo "======================================================================"
echo "Starting CCSDS Decoder Integration Smoke Test (Sprint 3)"
echo "======================================================================"

# 1. Config environment
export PROTOC="/home/admin-yash/Desktop/Decode/bin/bin/protoc"
export AMQP_URL="amqp://guest:guest@localhost:5672/%2f"
export SOURCE_EXCHANGE="telemetry.raw"
export SOURCE_QUEUE="ccsds-decoder.raw"
export SOURCE_ROUTING_KEY="#"
export DESTINATION_EXCHANGE="telemetry.decoded"
export CHECK_CRC="false"

# Create clean log files
DECODER_LOG="/tmp/decoder_smoke_test.log"
CONSUMER_LOG="/tmp/consumer_smoke_test.log"
rm -f "$DECODER_LOG" "$CONSUMER_LOG"
touch "$DECODER_LOG" "$CONSUMER_LOG"

# 2. Build the binaries first to avoid build time in running time
echo "Building binaries..."
cargo build --bin ccsds-decoder --bin publish-test-envelope --bin consume-decoded-envelope

# 3. Start the test consumer in the background
echo "Starting decoded envelope test consumer..."
cargo run --bin consume-decoded-envelope > "$CONSUMER_LOG" 2>&1 &
CONSUMER_PID=$!

# 4. Start the decoder in the background
echo "Starting decoder..."
cargo run --bin ccsds-decoder > "$DECODER_LOG" 2>&1 &
DECODER_PID=$!

# Cleanup trap to ensure background processes are always terminated
cleanup() {
  echo "Cleaning up processes..."
  if kill -0 $DECODER_PID 2>/dev/null; then
    kill $DECODER_PID
    wait $DECODER_PID 2>/dev/null || true
  fi
  if kill -0 $CONSUMER_PID 2>/dev/null; then
    kill $CONSUMER_PID
    wait $CONSUMER_PID 2>/dev/null || true
  fi
}
trap cleanup EXIT

# Wait for decoder & consumer to start up and connect to RabbitMQ
echo "Waiting for services to establish AMQP connections..."
sleep 4

# 5. Run the publish helper to send the packet
echo "Publishing test telemetry envelope..."
cargo run --bin publish-test-envelope

# Wait for consumption, processing, publishing and receipt
echo "Waiting for message pipeline processing..."
sleep 4

# 6. Stop the background processes
cleanup
trap - EXIT

echo "======================================================================"
echo "Verifying processed output in logs"
echo "======================================================================"

echo "--- DECODER LOGS ---"
cat "$DECODER_LOG"
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
check_file_log "$DECODER_LOG" "\[CCSDS ✓\]" "Decoder processed packet successfully"
check_file_log "$DECODER_LOG" "APID=  42" "Verified console log APID=42"
check_file_log "$DECODER_LOG" "Seq= 1200" "Verified console log Sequence=1200"

echo "=== Running Egress & Mutation Assertions ==="
check_file_log "$CONSUMER_LOG" "\[ASSERT_STAGE=2\]" "Verified stage mutated to CcsdsDecoded (2)"
check_file_log "$CONSUMER_LOG" "\[ASSERT_APID=42\]" "Verified envelope APID=42"
check_file_log "$CONSUMER_LOG" "\[ASSERT_HDR_APID=42\]" "Verified CcsdsPacketHeader APID=42"
check_file_log "$CONSUMER_LOG" "\[ASSERT_HDR_SEQ=1200\]" "Verified CcsdsPacketHeader Seq=1200"
check_file_log "$CONSUMER_LOG" "\[ASSERT_HDR_TYPE=1\]" "Verified CcsdsPacketHeader Type is TM (1)"
check_file_log "$CONSUMER_LOG" "\[ASSERT_CRC_OK=false\]" "Verified CRC OK flag is false"
check_file_log "$CONSUMER_LOG" "\[ASSERT_SEQ_CONT=true\]" "Verified Sequence Continuous flag is true"
check_file_log "$CONSUMER_LOG" "\[ASSERT_PUB_TS_SOURCE=4\]" "Verified publish timestamp source is System (4)"
check_file_log "$CONSUMER_LOG" "\[ASSERT_ROUTING_KEY=cy3.sat101.42.decoded\]" "Verified dynamic routing key is cy3.sat101.42.decoded"

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
