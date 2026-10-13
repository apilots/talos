FROM ubuntu:24.04
# Acceptance-only helper. Rules apply exclusively inside a fixture container's
# network namespace, never the Mac host, Docker host, or other containers.
# Read-only DB evidence also runs here, sharing the Linux SQLite locking domain.
RUN apt-get update && apt-get install -y --no-install-recommends iptables python3 \
    && rm -rf /var/lib/apt/lists/*
CMD ["sh", "-ec", "iptables -A INPUT -i lo -j ACCEPT; iptables -A INPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT; iptables -P INPUT DROP"]
