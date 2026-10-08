#!/usr/bin/env bash
set -euo pipefail
: "${FL_GIT_SSH_PRIVATE_KEY:?Missing Git SSH private key path}"
: "${FL_GIT_SSH_KNOWN_HOSTS:?Missing Git SSH known-hosts path}"
exec ssh -F /dev/null \
  -i "$FL_GIT_SSH_PRIVATE_KEY" \
  -o "UserKnownHostsFile=$FL_GIT_SSH_KNOWN_HOSTS" \
  -o GlobalKnownHostsFile=/dev/null \
  -o StrictHostKeyChecking=yes \
  -o IdentitiesOnly=yes \
  -o IdentityAgent=none \
  -o BatchMode=yes \
  -o ConnectTimeout=5 \
  -o ServerAliveInterval=10 \
  -o ServerAliveCountMax=1 \
  -o ClearAllForwardings=yes \
  "$@"
