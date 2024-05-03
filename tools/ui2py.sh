#!/bin/bash

pyuic6 ../src/spectroscope/spectroscope.ui -o ../src/spectroscope/ui_spectroscope.py
pyuic6 ../src/spectroscope/spectroscope_baseline.ui -o ../src/spectroscope/ui_spectroscope_baseline.py
pyuic6 ../src/spectroscope/spectroscope_colors.ui -o ../src/spectroscope/ui_spectroscope_colors.py
pyuic6 ../src/spectroscope/spectroscope_persistence.ui -o ../src/spectroscope/ui_spectroscope_persistence.py
pyuic6 ../src/spectroscope/spectroscope_settings_help.ui -o ../src/spectroscope/ui_spectroscope_settings_help.py
pyuic6 ../src/spectroscope/spectroscope_settings.ui -o ../src/spectroscope/ui_spectroscope_settings.py
pyuic6 ../src/spectroscope/spectroscope_smoothing.ui -o ../src/spectroscope/ui_spectroscope_smoothing.py

echo "Done!"
echo "Do anykey to exit..."
read
