#!/bin/bash

cd ../spectroscope/ || exit

pyuic6 spectroscope.ui -o ui_spectroscope.py
pyuic6 spectroscope_baseline.ui -o ui_spectroscope_baseline.py
pyuic6 spectroscope_colors.ui -o ui_spectroscope_colors.py
pyuic6 spectroscope_persistence.ui -o ui_spectroscope_persistence.py
pyuic6 spectroscope_settings_help.ui -o ui_spectroscope_settings_help.py
pyuic6 spectroscope_settings.ui -o ui_spectroscope_settings.py
pyuic6 spectroscope_smoothing.ui -o ui_spectroscope_smoothing.py

