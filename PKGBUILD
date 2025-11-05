#!/usr/bin/env bash
# Maintainer: Ali <you@example.com>

pkgname=turbo-git
_pkgname=aurwrap
pkgver=0.1.13.beta.2.gc7b483d
pkgrel=1
pkgdesc="Turbo: fast Rust AUR helper that wraps pacman for repo + AUR installs"
arch=('x86_64' 'aarch64')
url="https://github.com/splizer101/turbo"
license=('MIT' 'Apache-2.0')
depends=('pacman' 'git' 'openssl' 'nnn')
makedepends=('cargo' 'rust' 'pkgconf')
optdepends=('lf: alternative file manager'
            'neovim: default editor'
            'nano: alternative editor')
provides=('turbo')
conflicts=('turbo')
source=("${pkgname}::git+${url}.git")
sha256sums=('SKIP')
install=${pkgname}.install

pkgver() {
  cd "${srcdir}/${pkgname}"
  git describe --tags --long | sed 's/^v//;s/-/./g'
}

build() {
  cd "${srcdir}/${pkgname}"
  export RUSTFLAGS="-C target-cpu=native -C llvm-args=-cost-kind=latency -C opt-level=3 -C codegen-units=1"
  cargo build --release
}

check() {
  cd "${srcdir}/${pkgname}"
  cargo test
}

package() {
  cd "${srcdir}/${pkgname}"
  install -Dm755 "target/release/${_pkgname}" "${pkgdir}/usr/bin/turbo"
  install -Dm755 setup_turbo.sh "${pkgdir}/usr/share/turbo/setup-turbo"
  install -Dm755 turbo-fm "${pkgdir}/usr/share/turbo/turbo-fm"

  if [[ -f README.md ]]; then
    install -Dm644 README.md "${pkgdir}/usr/share/doc/turbo/README.md"
  fi
}
