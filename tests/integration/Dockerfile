# SPDX-FileCopyrightText: 2026 shellkeep contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Dockerfile for shellkeep integration tests.
# Provides sshd + tmux for real SSH connection testing.
#
# Build:
#   docker build -t shellkeep-test-sshd tests/integration/
#
# Run:
#   docker run -d -p 2222:22 --name sk-test-sshd shellkeep-test-sshd
#
# Connect:
#   ssh -p 2222 -o StrictHostKeyChecking=no testuser@localhost
#   Password: testpass

FROM debian:bookworm-slim

# Install sshd, tmux, and basic utilities.
RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        openssh-server \
        tmux \
        procps \
        ca-certificates \
        iptables \
        iproute2 \
    && rm -rf /var/lib/apt/lists/*

# Create SSH host keys.
RUN mkdir -p /run/sshd && \
    ssh-keygen -A

# Configure sshd:
# - Allow password authentication for tests.
# - Disable PAM to simplify container setup.
# - Enable SFTP subsystem.
RUN sed -i 's/^#PasswordAuthentication yes/PasswordAuthentication yes/' /etc/ssh/sshd_config && \
    sed -i 's/^#PermitRootLogin .*/PermitRootLogin no/' /etc/ssh/sshd_config && \
    sed -i 's/^UsePAM yes/UsePAM no/' /etc/ssh/sshd_config && \
    grep -q "^Subsystem.*sftp" /etc/ssh/sshd_config || echo "Subsystem sftp /usr/lib/openssh/sftp-server" >> /etc/ssh/sshd_config && \
    echo "ClientAliveInterval 10" >> /etc/ssh/sshd_config && \
    echo "ClientAliveCountMax 3" >> /etc/ssh/sshd_config

# Create test user with known password.
RUN useradd -m -s /bin/bash testuser && \
    echo 'testuser:testpass' | chpasswd

# Create .terminal-state directory structure for the test user.
RUN mkdir -p /home/testuser/.terminal-state/history && \
    mkdir -p /home/testuser/.terminal-state/state && \
    chown -R testuser:testuser /home/testuser/.terminal-state && \
    chmod 700 /home/testuser/.terminal-state

# Verify tmux is installed and meets version requirement (>= 3.0).
RUN tmux -V

# Expose SSH port.
EXPOSE 22

# Start sshd in the foreground.
CMD ["/usr/sbin/sshd", "-D", "-e"]
