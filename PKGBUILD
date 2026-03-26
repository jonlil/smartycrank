pkgname=smartycrank
pkgver=0.1.0
pkgrel=1
pkgdesc='Control your Samsung TV locally via WebSocket'
arch=('x86_64')
url='https://github.com/jonlil/smartycrank'
license=('MIT')
depends=('gcc-libs' 'openssl' 'dbus')
makedepends=('cargo')

build() {
  cd "$startdir"
  RUSTFLAGS="-C linker=gcc" cargo build --release --locked
}

package() {
  install -Dm755 "$startdir/target/release/smartycrank" "$pkgdir/usr/bin/smartycrank"
}
