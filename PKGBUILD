#!/usr/bin/env bash
# Maintainer: Ali <you@example.com>

pkgname=turbo-git
_pkgname=aurwrap
pkgver=0.1.0
pkgrel=1
pkgdesc="Turbo: AUR helper in Rust that wraps pacman (paru-like): edit, build in cache, single pacman -U"
arch=('x86_64' 'aarch64')
url="https://github.com/awesomeali101/turbo"
license=('MIT' 'Apache-2.0')
depends=('pacman' 'git' 'nnn')
makedepends=('cargo' 'rust')
optdepends=('lf: alternative file manager'
            'neovim: default editor'
            'nano: alternative editor')
provides=("turbo")
conflicts=("turbo")
source=("${pkgname}::git+${url}.git")
sha256sums=('SKIP')

pkgver() {
  cd "${srcdir}/${pkgname}"
  # Try git describe (tags), fallback to commit count + short hash
  if git describe --tags --long --always >/dev/null 2>&1; then
    # Convert v0.1.0-12-gabcdef0 -> 0.1.0.r12.gabcdef0
    git describe --tags --long --always \
      | sed -E 's/^v?([0-9]+\.[0-9]+\.[0-9]+)-([0-9]+)-g([0-9a-f]+)$/\1.r\2.g\3/'
  else
    printf "0.0.0.r%s.g%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
  fi
}

build() {
  cd "${srcdir}/${pkgname}"
  export RUSTFLAGS="-C target-cpu=native -C opt-level=3 -C codegen-units=1 -C llvm-args=-cost-kind=latency"
  cargo build --release --locked
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

  # Docs
  if [[ -f README.md ]]; then
    install -Dm644 README.md "${pkgdir}/usr/share/doc/turbo/README.md"
  fi
}
