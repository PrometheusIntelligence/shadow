#!/usr/bin/env bash
# Regenerates scripts/hive-lockdown.sh's PEERS line from
# ansible/inventory/hosts.ini -- SOURCE OF TRUTH, matching the same
# awk-derivation style scripts/deploy-ui-fleet.sh already uses for its host
# list, so the peer roster is never hand-copied (and never drifts) again.
#
# Usage: scripts/gen-hive-lockdown.sh   (rewrites scripts/hive-lockdown.sh in place)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INVENTORY="$REPO_ROOT/ansible/inventory/hosts.ini"
TARGET="$REPO_ROOT/scripts/hive-lockdown.sh"
[ -f "$INVENTORY" ] || { echo "inventory not found: $INVENTORY" >&2; exit 1; }

# A host marked `hive_retired=true` (its instance no longer exists) is left
# OUT: a cloud provider re-issues a released public IP to other customers, so
# keeping it would hand a stranger peer-level access through every node's
# firewall. Witnessed 2026-09-30: 18 of 24 trusted IPs belonged to instances
# the account no longer held.
roster() { # $1 = inventory var holding the address to emit
  awk -v key="$1" '
    /^[[:space:]]*#/ { next }
    /ansible_host=/ {
      addr = ""; retired = 0
      for (i = 1; i <= NF; i++) {
        split($i, a, "=")
        if (a[1] == key) addr = a[2]
        if (a[1] == "hive_retired" && a[2] == "true") retired = 1
      }
      if (addr != "" && !retired) print addr
    }
  ' "$INVENTORY"
}
mapfile -t PEER_IPS < <(roster ansible_host)
mapfile -t PRIVATE_IPS < <(roster hive_private_ip)
[ ${#PEER_IPS[@]} -gt 0 ] || { echo "no live ansible_host entries parsed from $INVENTORY" >&2; exit 1; }

PEERS_LINE="${PEER_IPS[*]}"
PRIVATE_LINE="${PRIVATE_IPS[*]:-}"

python3 - "$TARGET" "$PEERS_LINE" "$PRIVATE_LINE" <<'PYEOF'
import re, sys
target, peers_line, private_line = sys.argv[1], sys.argv[2], sys.argv[3]
content = open(target).read()
for var, value in (("PEERS", peers_line), ("PRIVATE_PEERS", private_line)):
    content, n = re.subn(rf'^{var}="[^"]*"', f'{var}="{value}"', content, count=1, flags=re.MULTILINE)
    if n == 0:
        print(f"ERROR: {var}= line not found in {target}", file=sys.stderr)
        sys.exit(1)
open(target, "w").write(content)
print(f"wrote {len(peers_line.split())} public and {len(private_line.split())} private peers to {target}")
PYEOF
