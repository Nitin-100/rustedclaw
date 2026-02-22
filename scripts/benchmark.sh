#!/usr/bin/env bash
#
# RustedClaw Benchmark Script — Linux / macOS
#
# Usage:
#   chmod +x scripts/benchmark.sh
#   ./scripts/benchmark.sh [path-to-rustedclaw-binary]
#
# If no path is given, it looks for ./target/release/rustedclaw
#

set -euo pipefail

BINARY="${1:-./target/release/rustedclaw}"
PORT=42617
REQUESTS=200
RESULTS_FILE="benchmark-results.txt"

# ── Colors ─────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

header() { echo -e "\n${BOLD}${CYAN}═══ $1 ═══${NC}"; }
metric() { echo -e "  ${GREEN}✓${NC} $1: ${BOLD}$2${NC}"; }
fail()   { echo -e "  ${RED}✗${NC} $1"; }

echo -e "${BOLD}"
echo "  ┌──────────────────────────────────────────┐"
echo "  │   🦀  RustedClaw Benchmark Suite          │"
echo "  │       Linux / macOS                       │"
echo "  └──────────────────────────────────────────┘"
echo -e "${NC}"

# ── Check binary ───────────────────────────────────────────────────────────
if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Binary not found at: $BINARY${NC}"
    echo "Build first: cargo build --release"
    echo "Or pass path: ./scripts/benchmark.sh /path/to/rustedclaw"
    exit 1
fi

# ── 1. Binary Size ─────────────────────────────────────────────────────────
header "Binary Size"
BIN_SIZE_BYTES=$(stat -f%z "$BINARY" 2>/dev/null || stat -c%s "$BINARY" 2>/dev/null)
BIN_SIZE_MB=$(echo "scale=2; $BIN_SIZE_BYTES / 1048576" | bc)
metric "Binary size" "${BIN_SIZE_MB} MB ($BIN_SIZE_BYTES bytes)"

# ── 2. Cold Start Time ────────────────────────────────────────────────────
header "Cold Start Time"
TOTAL_MS=0
RUNS=10
for i in $(seq 1 $RUNS); do
    START=$(date +%s%N 2>/dev/null || python3 -c "import time; print(int(time.time()*1e9))")
    "$BINARY" --version > /dev/null 2>&1
    END=$(date +%s%N 2>/dev/null || python3 -c "import time; print(int(time.time()*1e9))")
    ELAPSED=$(( (END - START) / 1000000 ))
    TOTAL_MS=$((TOTAL_MS + ELAPSED))
done
AVG_MS=$((TOTAL_MS / RUNS))
metric "Cold start (avg of $RUNS runs)" "${AVG_MS} ms"

# ── 3. Start Gateway (background) ─────────────────────────────────────────
header "Starting Gateway (port $PORT)"

# Kill any existing instance
pkill -f "rustedclaw.*gateway" 2>/dev/null || true
sleep 1

"$BINARY" gateway --port $PORT > /dev/null 2>&1 &
GW_PID=$!
sleep 2

# Verify it's running
if ! kill -0 "$GW_PID" 2>/dev/null; then
    fail "Gateway failed to start"
    exit 1
fi
metric "Gateway PID" "$GW_PID"

# ── 4. Idle Memory ────────────────────────────────────────────────────────
header "Idle Memory"

if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    RSS_KB=$(ps -o rss= -p "$GW_PID" | tr -d ' ')
    VSZ_KB=$(ps -o vsz= -p "$GW_PID" | tr -d ' ')
    RSS_MB=$(echo "scale=2; $RSS_KB / 1024" | bc)
    VSZ_MB=$(echo "scale=2; $VSZ_KB / 1024" | bc)
    CPU_PCT=$(ps -o %cpu= -p "$GW_PID" | tr -d ' ')
    THREADS=$(ps -M "$GW_PID" 2>/dev/null | wc -l | tr -d ' ')
else
    # Linux
    RSS_KB=$(awk '/VmRSS/{print $2}' /proc/$GW_PID/status 2>/dev/null || ps -o rss= -p "$GW_PID" | tr -d ' ')
    VSZ_KB=$(awk '/VmSize/{print $2}' /proc/$GW_PID/status 2>/dev/null || ps -o vsz= -p "$GW_PID" | tr -d ' ')
    RSS_MB=$(echo "scale=2; $RSS_KB / 1024" | bc)
    VSZ_MB=$(echo "scale=2; $VSZ_KB / 1024" | bc)
    CPU_PCT=$(ps -o %cpu= -p "$GW_PID" | tr -d ' ')
    THREADS=$(awk '/Threads/{print $2}' /proc/$GW_PID/status 2>/dev/null || echo "N/A")
fi

metric "RSS (Resident Set)" "${RSS_MB} MB"
metric "Virtual Memory" "${VSZ_MB} MB"
metric "CPU %" "${CPU_PCT}%"
metric "Threads" "$THREADS"

# ── 5. Load Test ──────────────────────────────────────────────────────────
header "Load Test ($REQUESTS requests)"

BASE_URL="http://127.0.0.1:$PORT"
FAILED=0
PASSED=0

# Warm up
curl -s "$BASE_URL/health" > /dev/null 2>&1

# Timed burst
START_LOAD=$(date +%s%N 2>/dev/null || python3 -c "import time; print(int(time.time()*1e9))")

for i in $(seq 1 $REQUESTS); do
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/v1/status" 2>/dev/null || echo "000")
    if [ "$STATUS" = "200" ]; then
        PASSED=$((PASSED + 1))
    else
        FAILED=$((FAILED + 1))
    fi
done

END_LOAD=$(date +%s%N 2>/dev/null || python3 -c "import time; print(int(time.time()*1e9))")
LOAD_MS=$(( (END_LOAD - START_LOAD) / 1000000 ))
LOAD_SEC=$(echo "scale=2; $LOAD_MS / 1000" | bc)
RPS=$(echo "scale=1; $REQUESTS * 1000 / $LOAD_MS" | bc)

metric "Requests" "$PASSED passed, $FAILED failed"
metric "Duration" "${LOAD_SEC}s"
metric "Throughput" "${RPS} req/sec"

# ── 6. Memory After Load ─────────────────────────────────────────────────
header "Memory After Load"

if [[ "$OSTYPE" == "darwin"* ]]; then
    RSS_AFTER_KB=$(ps -o rss= -p "$GW_PID" | tr -d ' ')
    RSS_AFTER_MB=$(echo "scale=2; $RSS_AFTER_KB / 1024" | bc)
    CPU_AFTER=$(ps -o %cpu= -p "$GW_PID" | tr -d ' ')
else
    RSS_AFTER_KB=$(awk '/VmRSS/{print $2}' /proc/$GW_PID/status 2>/dev/null || ps -o rss= -p "$GW_PID" | tr -d ' ')
    RSS_AFTER_MB=$(echo "scale=2; $RSS_AFTER_KB / 1024" | bc)
    CPU_AFTER=$(ps -o %cpu= -p "$GW_PID" | tr -d ' ')
fi

GROWTH=$(echo "scale=2; $RSS_AFTER_MB - $RSS_MB" | bc)
metric "RSS after load" "${RSS_AFTER_MB} MB"
metric "Memory growth" "${GROWTH} MB"
metric "CPU % after load" "${CPU_AFTER}%"

# ── 7. API Endpoint Validation ────────────────────────────────────────────
header "API Endpoint Validation"

ENDPOINTS=(
    "GET:/health"
    "GET:/v1/status"
    "GET:/v1/tools"
    "GET:/v1/conversations"
    "GET:/v1/routines"
    "GET:/v1/memory?q=test"
    "GET:/v1/jobs"
    "GET:/v1/config"
    "GET:/"
    "GET:/static/style.css"
    "GET:/static/app.js"
)

EP_PASS=0
EP_FAIL=0
for EP in "${ENDPOINTS[@]}"; do
    METHOD="${EP%%:*}"
    PATH="${EP#*:}"
    STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X "$METHOD" "$BASE_URL$PATH" 2>/dev/null || echo "000")
    if [ "$STATUS" = "200" ]; then
        metric "$METHOD $PATH" "$STATUS OK"
        EP_PASS=$((EP_PASS + 1))
    else
        fail "$METHOD $PATH → $STATUS"
        EP_FAIL=$((EP_FAIL + 1))
    fi
done

echo ""
metric "Endpoints" "$EP_PASS passed, $EP_FAIL failed"

# ── Cleanup ───────────────────────────────────────────────────────────────
header "Cleanup"
kill "$GW_PID" 2>/dev/null || true
wait "$GW_PID" 2>/dev/null || true
metric "Gateway stopped" "PID $GW_PID"

# ── Summary ───────────────────────────────────────────────────────────────
echo ""
echo -e "${BOLD}${CYAN}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${BOLD}${CYAN}║        🦀 RustedClaw Benchmark Summary           ║${NC}"
echo -e "${BOLD}${CYAN}╠══════════════════════════════════════════════════╣${NC}"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Binary Size:" "${BIN_SIZE_MB} MB"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Cold Start:" "${AVG_MS} ms"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Idle RAM:" "${RSS_MB} MB"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "RAM After Load:" "${RSS_AFTER_MB} MB"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Memory Growth:" "${GROWTH} MB"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Throughput:" "${RPS} req/sec"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Threads:" "$THREADS"
printf "${CYAN}║${NC}  %-22s %24s ${CYAN}║${NC}\n" "Endpoints:" "$EP_PASS/$((EP_PASS + EP_FAIL)) OK"
echo -e "${BOLD}${CYAN}╚══════════════════════════════════════════════════╝${NC}"

# Save results to file
cat > "$RESULTS_FILE" << EOF
RustedClaw Benchmark Results
$(date -u +"%Y-%m-%d %H:%M:%S UTC")
$(uname -srm)

Binary Size:      ${BIN_SIZE_MB} MB
Cold Start:       ${AVG_MS} ms (avg of $RUNS runs)
Idle RAM (RSS):   ${RSS_MB} MB
RAM After Load:   ${RSS_AFTER_MB} MB
Memory Growth:    ${GROWTH} MB
Load Test:        $REQUESTS requests in ${LOAD_SEC}s (${RPS} req/s)
Threads:          $THREADS
Endpoints:        $EP_PASS/$((EP_PASS + EP_FAIL)) OK
EOF

echo ""
echo -e "Results saved to ${BOLD}$RESULTS_FILE${NC}"
