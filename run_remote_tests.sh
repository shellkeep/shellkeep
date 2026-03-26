#!/usr/bin/env bash
# Script to run all tests on the remote droplet
set -euo pipefail

SSH_KEY="/home/node/.ssh/id_shellkeep"
REMOTE="root@209.38.150.61"
SSH_OPTS="-i $SSH_KEY -o StrictHostKeyChecking=no -o LogLevel=ERROR"

run_remote() {
    ssh $SSH_OPTS "$REMOTE" "$@"
}

echo "=== Step 1: Fix dpkg and install prerequisites ==="
run_remote 'dpkg --configure -a 2>&1; apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq docker.io sshpass iproute2 iptables jq tmux openssh-client 2>&1 | tail -5'

echo "=== Step 2: Ensure Docker is running ==="
run_remote 'systemctl start docker 2>/dev/null || service docker start 2>/dev/null || dockerd &>/dev/null &'
sleep 2
run_remote 'docker info 2>&1 | head -3'

echo "=== Step 3: Build integration test Docker image ==="
run_remote 'cd /opt/shellkeep-e2e/tests/integration && docker build -t shellkeep-test-sshd . 2>&1 | tail -5'

echo "=== Step 4: Build chaos Docker image ==="
run_remote 'cd /opt/shellkeep-e2e/tests/chaos && docker build -t shellkeep-chaos-sshd . 2>&1 | tail -5'

echo "=== Step 5: Build first-run Docker images ==="
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && docker build -t sk-fr-full -f ../integration/Dockerfile ../integration/ 2>&1 | tail -3'
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && docker build -t sk-fr-no-tmux -f Dockerfile.no-tmux . 2>&1 | tail -3'
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && docker build -t sk-fr-tmux2 -f Dockerfile.tmux2 . 2>&1 | tail -3'

echo "=== Step 6: Run E2E tests ==="
run_remote 'cd /opt/shellkeep-e2e/tests/e2e && bash run_all.sh 2>&1 | tee /tmp/e2e-results.txt; echo "EXIT_CODE=$?"'

echo "=== Step 7: Run chaos tests (skip long) ==="
run_remote 'cd /opt/shellkeep-e2e/tests/chaos && bash run_all.sh --skip-long 2>&1 | tee /tmp/chaos-results.txt; echo "EXIT_CODE=$?"'

echo "=== Step 8: Run multi-client tests ==="
run_remote 'cd /opt/shellkeep-e2e/tests/multi-client && bash run_all.sh 2>&1 | tee /tmp/multi-client-results.txt; echo "EXIT_CODE=$?"'

echo "=== Step 9: Run first-run tests ==="
run_remote 'cd /opt/shellkeep-e2e/tests/first-run && bash run_all.sh 2>&1 | tee /tmp/first-run-results.txt; echo "EXIT_CODE=$?"'

echo "=== All test suites completed ==="
