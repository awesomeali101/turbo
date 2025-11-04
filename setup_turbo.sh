#!/usr/bin/env bash
set -euo pipefail

resolve_user_home() {
  local u=""
  # 1) sudo path (most common)
  if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
    u="$SUDO_USER"
  # 2) pkexec
  elif [[ -n "${PKEXEC_UID:-}" ]]; then
    u="$(id -nu "$PKEXEC_UID" 2>/dev/null || true)"
  fi

  # 3) interactive installer: fallback to TTY owner
  if [[ -z "$u" || "$u" == "root" ]]; then
    u="$(logname 2>/dev/null || true)"
  fi

  # 4) final fallback: first “real” user (uid ≥ 1000) with an existing home
  if [[ -z "$u" || "$u" == "root" ]]; then
    u="$(awk -F: '$3>=1000 && $1!="nobody"{print $1; exit}' /etc/passwd)"
  fi

  # Map to home
  local h
  h="$(getent passwd "$u" | cut -d: -f6)"
  if [[ -z "$h" || ! -d "$h" ]]; then
    # last resort: root
    u="root"
    h="/root"
  fi

  USER_NAME="$u"
  USER_HOME="$h"
}

# Use it:
resolve_user_home
ROOT_DIR="${USER_HOME}/turbo"
CACHE_DIR="${ROOT_DIR}/cache"
TEMP_DIR="${CACHE_DIR}/temp"
CONF_FILE="${ROOT_DIR}/conf"
mkdir -p "${TEMP_DIR}"
LOCAL_DIR="/usr/share/turbo"
sudo cp "${LOCAL_DIR}/turbo-fm" /usr/bin/turbo-fm

if [[ ! -f "${CONF_FILE}" ]]; then
  cat >"${CONF_FILE}" <<'EOF'
# turbo configuration (simple key=value)
# editor: nvim | nano | ...
editor=nvim
# file_manager: nnn | lf | ...
file_manager=nnn
# mirror: aur | github
mirror=aur
# mirror_base: when mirror=github, base URL for repos
# mirror_base=https://github.com/archlinux-aur
EOF

  echo "Created default conf at ${CONF_FILE}"
else
  echo "Conf already exists at ${CONF_FILE}"
fi

echo "Ensured directories: ${TEMP_DIR}"
