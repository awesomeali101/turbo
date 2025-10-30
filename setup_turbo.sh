#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${HOME}/turbo"
CACHE_DIR="${ROOT_DIR}/cache"
TEMP_DIR="${CACHE_DIR}/temp"
CONF_FILE="${ROOT_DIR}/conf"

mkdir -p "${TEMP_DIR}"
sudo cp "./turbo-fm" /usr/bin/turbo-fm

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
