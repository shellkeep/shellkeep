#!/usr/bin/env bash
# Full E2E test runner for shellkeep - runs all test suites on the remote test droplet
# This script: uploads code, installs prerequisites, builds Docker images, and runs all tests
set -euo pipefail

SSH_KEY="/home/node/.ssh/id_shellkeep"
REMOTE="root@209.38.150.61"
SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no -o LogLevel=ERROR"

run_remote() {
    ssh $SSH_OPTS "$REMOTE" "$@"
}

upload_project() {
    tar czf - --exclude='.git' -C /workspace . | \
        ssh $SSH_OPTS "$REMOTE" 'mkdir -p /opt/shellkeep-e2e && cd /opt/shellkeep-e2e && tar xzf -'
}

echo "=== Uploading project ==="
upload_project

echo "=== Installing prerequisites ==="
run_remote 'dpkg --configure -a 2>/dev/null; apt-get update -qq 2>/dev/null; DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker.io sshpass iproute2 iptables jq tmux 2>&1 | tail -3'

echo "=== Starting Docker ==="
run_remote 'systemctl start docker 2>/dev/null || service docker start 2>/dev/null; sleep 2; docker info 2>&1 | head -2'

echo "=== Building Docker images ==="
run_remote 'cd /opt/shellkeep-e2e/tests/integration && docker build -t shellkeep-test-sshd . 2>&1 | tail -3'
run_remote 'cd /opt/shellkeep-e2e/tests/chaos && docker build -t shellkeep-chaos-sshd . 2>&1 | tail -3'
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && docker build -t sk-fr-full -f ../integration/Dockerfile ../integration/ 2>&1 | tail -3'
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && docker build -t sk-fr-no-tmux -f Dockerfile.no-tmux . 2>&1 | tail -3'
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && docker build -t sk-fr-tmux2 -f Dockerfile.tmux2 . 2>&1 | tail -3'

echo ""
echo "============================================"
echo "  Running E2E Tests"
echo "============================================"
run_remote 'cd /opt/shellkeep-e2e/tests/e2e && bash run_all.sh 2>&1' | tee /tmp/e2e-results.txt
E2E_RC=${PIPESTATUS[0]}

echo ""
echo "============================================"
echo "  Running Chaos Tests (--skip-long)"
echo "============================================"
run_remote 'cd /opt/shellkeep-e2e/tests/chaos && bash run_all.sh --skip-long 2>&1' | tee /tmp/chaos-results.txt
CHAOS_RC=${PIPESTATUS[0]}

echo ""
echo "============================================"
echo "  Running Multi-Client Tests"
echo "============================================"
run_remote 'cd /opt/shellkeep-e2e/tests/multi-client && bash run_all.sh 2>&1' | tee /tmp/multi-client-results.txt
MC_RC=${PIPESTATUS[0]}

echo ""
echo "============================================"
echo "  Running First-Run Tests"
echo "============================================"
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && bash run_all.sh 2>&1' | tee /tmp/first-run-results.txt
FR_RC=${PIPESTATUS[0]}

echo ""
echo "============================================"
echo "  OVERALL RESULTS"
echo "============================================"
echo "  E2E:          $([ $E2E_RC -eq 0 ] && echo PASS || echo FAIL)"
echo "  Chaos:        $([ $CHAOS_RC -eq 0 ] && echo PASS || echo FAIL)"
echo "  Multi-Client: $([ $MC_RC -eq 0 ] && echo PASS || echo FAIL)"
echo "  First-Run:    $([ $FR_RC -eq 0 ] && echo PASS || echo FAIL)"
echo "============================================"

if [[ $E2E_RC -ne 0 ]] || [[ $CHAOS_RC -ne 0 ]] || [[ $MC_RC -ne 0 ]] || [[ $FR_RC -ne 0 ]]; then
    exit 1
fi
exit 0
