#!/usr/bin/env bash
# Maintainer: Ali <you@example.com>

pkgname=turbo-git
# Optional prerelease suffix for version (letters/dots only, no hyphen). Example: _pre=beta
_pre=""
_pkgname=aurwrap
pkgver=aa2453b
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
  # Try git describe (tags), fallback to commit count + short hash
  if git describe --tags --long --always >/dev/null 2>&1; then
    # Convert v0.1.0-12-gabcdef0 -> 0.1.0[pre].r12.gabcdef0
    ver=$(git describe --tags --long --always \
      | sed -E 's/^v?([0-9]+\.[0-9]+\.[0-9]+)-([0-9]+)-g([0-9a-f]+)$/\1.r\2.g\3/')
  else
    ver=$(printf "0.0.0.r%s.g%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)")
  fi
  if [[ -n "${_pre}" ]]; then
    # Insert suffix right after the semantic version prefix
    printf "%s\n" "$ver" | sed -E "s/^([0-9]+(\.[0-9]+){1,2})/\1${_pre}/"
  else
    printf "%s\n" "$ver"
  fi
}

build() {
  cd "${srcdir}/${pkgname}"
  export RUSTFLAGS="-C target-cpu=native -C opt-level=3 -C codegen-units=1"
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
