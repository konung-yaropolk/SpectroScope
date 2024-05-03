echo off

pyuic6 ..\spectroscope\spectroscope.ui -o ..\spectroscope\ui_spectroscope.py
pyuic6 ..\spectroscope\spectroscope_baseline.ui -o ..\spectroscope\ui_spectroscope_baseline.py
pyuic6 ..\spectroscope\spectroscope_colors.ui -o ..\spectroscope\ui_spectroscope_colors.py
pyuic6 ..\spectroscope\spectroscope_persistence.ui -o ..\spectroscope\ui_spectroscope_persistence.py
pyuic6 ..\spectroscope\spectroscope_settings_help.ui -o ..\spectroscope\ui_spectroscope_settings_help.py
pyuic6 ..\spectroscope\spectroscope_settings.ui -o ..\spectroscope\ui_spectroscope_settings.py
pyuic6 ..\spectroscope\spectroscope_smoothing.ui -o ..\spectroscope\ui_spectroscope_smoothing.py

echo Done!
pause
