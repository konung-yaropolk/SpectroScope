pkgname=spectroscope
pkgver=0.2.2
pkgrel=1
pkgdesc="Spectrum analyzer for multiple SDR platforms (PyQtGraph based GUI for soapy_power, hackrf_sweep, rtl_power, rx_power and other backends)"
arch=('any')
url="https://github.com/konung-yaropolk/spectroscope"
license=('GPL3')
depends=('python' 'python-pyqt6' 'python-pyqtgraph' 'soapy_power>=1.6.0')
makedepends=('python-setuptools')
optdepends=(
  'hackrf: hackrf_sweep backend (wideband spectrum monitoring with sweep rate of 8 GHz/s)'
  'rtl_power_fftw-git: alternative RTL-SDR backend using FFTW library (much faster than rtl_power)'
  'rtl-sdr-keenerd-git: better version of rtl_power backend'
  'rtl-sdr: original rtl_power backend (slightly broken, use rtl-sdr-keenerd-git instead)'
  'rx_tools: rx_power backend (universal SoapySDR based backend, but seems slow and buggy)'
)
source=(https://github.com/konung-yaropolk/spectroscope/archive/v$pkgver.tar.gz)

build() {
  cd "$srcdir/$pkgname-$pkgver"
  python setup.py build
}

package() {
  cd "$srcdir/$pkgname-$pkgver"
  python setup.py install --root="$pkgdir"
}

