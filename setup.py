#!/usr/bin/env python

import setuptools
from setuptools import setup

from spectroscope.version import __version__

setup_cmdclass = {}
setup_entry_points = {
    "gui_scripts": [
        "spectroscope=spectroscope.__main__:main",
    ],
}

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
    name="SpectroScope",
    version=__version__,
    description=("Spectrum analyzer for multiple SDR platforms "
                 "(PyQtGraph based GUI for soapy_power, hackrf_sweep, rtl_power, rx_power and other backends)"),
    long_description=open('README.rst').read(),
    author="konung-yaropolk",
    author_email="yaropolk1995@gmail.com",
    url="https://github.com/konung-yaropolk/spectroscope",
    license="GNU GPLv3",
    packages=["spectroscope", "spectroscope.backends"],
    package_data={
        "spectroscope": [
            "spectroscope.svg",
            "*.ui",
            "languages/*.qm",
            "languages/*.ts",
        ],
    },
    data_files=[
        ("share/applications", ["spectroscope.desktop"]),
        ("share/pixmaps", ["spectroscope.png"]),
    ],
    install_requires=[
        "soapy_power>=1.6.0",
        "pyqtgraph>=0.10.0",
        "qtpy",
    ],
    options={
        'build_qt': {
            'packages': ['spectroscope'],
            'languages': ['cs'],
            'replacement_bindings': 'Qt',
        },
        'build_exe': {
            'datas': [
                ('spectroscope/spectroscope.svg', 'spectroscope'),
                ('spectroscope/*.ui', 'spectroscope'),
                ('spectroscope/languages/*.ts', 'spectroscope/languages'),
                ('spectroscope/languages/*.qm', 'spectroscope/languages'),
                ('README.rst', '.'),
                ('LICENSE', '.'),
            ],
        },
        'bdist_msi': {
            'upgrade_code': '30740ef4-84e7-4e67-8e4a-12b53492c387',
            'shortcuts': [
                'SpectroScope=spectroscope',
            ],
        },
    },
    classifiers=[
        "Development Status :: 4 - Beta",
        "Environment :: MacOS X",
        "Environment :: Win32 (MS Windows)",
        "Environment :: X11 Applications :: Qt",
        "Intended Audience :: End Users/Desktop",
        "Intended Audience :: Science/Research",
        "Intended Audience :: Telecommunications Industry",
        "License :: OSI Approved :: GNU General Public License v3 or later (GPLv3+)",
        "Natural Language :: English",
        "Operating System :: OS Independent",
        "Programming Language :: Python :: 3",
        "Topic :: Communications :: Ham Radio",
        "Topic :: Scientific/Engineering :: Visualization",
    ],
    entry_points=setup_entry_points,
    cmdclass=setup_cmdclass,
)
