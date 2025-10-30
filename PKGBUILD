#!/usr/bin/env bash
# Maintainer: Ali <you@example.com>

pkgname=turbo-git
# Optional prerelease suffix for version (letters/dots only, no hyphen). Example: _pre=beta
_pre=""
_pkgname=aurwrap
pkgver=0.1.r6.gfe169ba
pkgrel=1
pkgdesc="Turbo: AUR helper in Rust that wraps pacman (paru-like): edit, build in cache, single pacman -U"
arch=('x86_64' 'aarch64')
url="https://github.com/awesomeali101/turbo"
license=('MIT' 'Apache-2.0')
depends=('pacman' 'git' 'nnn' 'openssl')
makedepends=('cargo' 'rust' 'pkgconf')
optdepends=('lf: alternative file manager'
            'neovim: default editor'
            'nano: alternative editor')
provides=("turbo")
conflicts=("turbo")
source=("${pkgname}::git+${url}.git")
sha256sums=('SKIP')

pkgver() {
  cd "${srcdir}/${pkgname}"
  # Preserve prefix of declared pkgver (everything before the last dot); only update the last segment
  local prefix rev hash tag
  prefix="${pkgver%.*}"
  if [[ -n "${_pre}" ]]; then
    prefix="${prefix}${_pre}"
  fi
  if tag=$(git describe --tags --abbrev=0 2>/dev/null); then
    rev=$(git rev-list --count "${tag}"..HEAD)
  else
    rev=$(git rev-list --count HEAD)
  fi
  hash=$(git rev-parse --short HEAD)
  printf "%s.r%s.g%s\n" "${prefix}" "${rev}" "${hash}"
}

build() {
  cd "${srcdir}/${pkgname}"
  export RUSTFLAGS="-C target-cpu=native -C llvm-args=-cost-kind=latency -C opt-level=3 -C codegen-units=1"
  cargo build --release
}

check() {
  cd "${srcdir}/${pkgname}"
  :
}

package() {
  cd "${srcdir}/${pkgname}"

  # Install main binary as /usr/bin/turbo (crate bin name is aurwrap)
  install -Dm755 "target/release/${_pkgname}" "${pkgdir}/usr/bin/turbo"

  # Install the file-manager wrapper if present
  if [[ -f turbo-fm ]]; then
    install -Dm755 turbo-fm "${pkgdir}/usr/bin/turbo-fm"
  fi

  # Optional setup script
  if [[ -f setup_turbo.sh ]]; then
    install -Dm755 setup_turbo.sh "${pkgdir}/usr/share/turbo/setup_turbo.sh"
  fi
  ./setup_turbo.sh

  # Docs
  if [[ -f README.md ]]; then
    install -Dm644 README.md "${pkgdir}/usr/share/doc/turbo/README.md"
  fi
}
