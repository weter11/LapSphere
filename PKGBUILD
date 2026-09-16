# Maintainer: LapSphere Team <hungaryam@gmail.com>
pkgname=lapsphere
pkgver=0.1.0
pkgrel=1
pkgdesc="Hardware control application for Uniwill/Clevo laptops"
arch=('x86_64')
url="https://github.com/weter11/lapsphere"
license=('GPL2')
depends=('dbus' 'polkit' 'libxkbcommon-x11' 'dmidecode' 'pciutils' 'ethtool' 'iw' 'gtk3' 'libadwaita')
optdepends=('optimus-manager: GPU switching support on Arch Linux')
makedepends=('cargo' 'cmake' 'gcc' 'pkgconf')
source=("lapsphere-$pkgver.tar.gz") # This will be handled by the CI or manual packaging
sha256sums=('SKIP')

build() {
  cd "$pkgname-$pkgver"

  # libmimalloc-sys builds its vendored C library (mimalloc v3, single TU
  # `static.c`) through the `cc` crate, which merges the ambient CFLAGS into
  # that compile. On the Arch job the resulting archive links but exports no
  # mi_* symbols, failing at final link with exactly:
  #   undefined symbol: mi_malloc_aligned / mi_realloc_aligned /
  #                    mi_zalloc_aligned / mi_free
  # The Ubuntu job builds the same source with no ambient CFLAGS and links
  # cleanly, so restore that condition here rather than inheriting the distro
  # flags. NOTE: the local toolchain (gcc 13) cannot reproduce the failure, so
  # this is targeted mitigation of the proven mechanism, not a locally
  # reproduced fix — the Arch CI run is the real verification.
  export CFLAGS="-O2 -pipe"
  export CXXFLAGS="-O2 -pipe"

  cargo build --release --all
}

package() {
  cd "$pkgname-$pkgver"
  # Binaries
  install -Dm755 target/release/lapsphere-daemon "$pkgdir/usr/bin/lapsphere-daemon"
  install -Dm755 target/release/lapsphere "$pkgdir/usr/bin/lapsphere"

  # DBus
  install -Dm644 data/io.lapsphere.Control.conf "$pkgdir/usr/share/dbus-1/system.d/io.lapsphere.Control.conf"
  install -Dm644 data/io.lapsphere.Control.service "$pkgdir/usr/share/dbus-1/system-services/io.lapsphere.Control.service"

  # systemd unit: supervision + resource caps for the daemon
  install -Dm644 data/lapsphere-daemon.service "$pkgdir/usr/lib/systemd/system/lapsphere-daemon.service"

  # Desktop & Autostart
  install -Dm644 data/io.lapsphere.LapSphere.desktop "$pkgdir/usr/share/applications/io.lapsphere.LapSphere.desktop"
  install -Dm644 data/io.lapsphere.LapSphere.desktop "$pkgdir/etc/xdg/autostart/io.lapsphere.LapSphere.desktop"
  sed -i 's/Exec=lapsphere/Exec=lapsphere --tray/' "$pkgdir/etc/xdg/autostart/io.lapsphere.LapSphere.desktop"

  # Icon
  install -Dm644 data/icon.svg "$pkgdir/usr/share/icons/hicolor/scalable/apps/lapsphere.svg"
}
