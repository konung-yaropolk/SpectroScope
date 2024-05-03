echo off

pyuic6 ..\src\spectroscope\ui\spectroscope.ui -o ..\src\spectroscope\ui\ui_spectroscope.py
pyuic6 ..\src\spectroscope\ui\spectroscope_baseline.ui -o ..\src\spectroscope\ui\ui_spectroscope_baseline.py
pyuic6 ..\src\spectroscope\ui\spectroscope_colors.ui -o ..\src\spectroscope\ui\ui_spectroscope_colors.py
pyuic6 ..\src\spectroscope\ui\spectroscope_persistence.ui -o ..\src\spectroscope\ui\ui_spectroscope_persistence.py
pyuic6 ..\src\spectroscope\ui\spectroscope_settings_help.ui -o ..\src\spectroscope\ui\ui_spectroscope_settings_help.py
pyuic6 ..\src\spectroscope\ui\spectroscope_settings.ui -o ..\src\spectroscope\ui\ui_spectroscope_settings.py
pyuic6 ..\src\spectroscope\ui\spectroscope_smoothing.ui -o ..\src\spectroscope\ui\ui_spectroscope_smoothing.py

echo Done!
pause
