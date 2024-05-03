#!/usr/bin/env python

from setuptools import setup


exec(open('src/spectroscope/version.py').read())

setup_cmdclass = {}

# Allow compilation of Qt .qrc, .ui and .ts files (build_qt command)
try:
    from setup_qt import build_qt
    setup_cmdclass['build_qt'] = build_qt
except ImportError:
    pass

# Allow building frozen executables with PyInstaller / subzero (build_exe command)
try:
    from subzero import setup, Executable
    setup_entry_points = {
        "console_scripts": [
            Executable('spectroscope=spectroscope.__main__:main',
                       console=True, icon_file='spectroscope.ico'),
            Executable('soapy_power=soapypower.__main__:main',
                       console=True),
        ],
    }
except ImportError:
    pass


setup(
    version = __version__,
    # cmdclass = setup_cmdclass,
)
