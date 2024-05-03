#!/usr/bin/env pwsh
Set-Location src\spectroscope\ui

Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope.ui -o ui_spectroscope.py" -NoNewWindow -Wait
Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope_baseline.ui -o ui_spectroscope_baseline.py" -NoNewWindow -Wait
Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope_colors.ui -o ui_spectroscope_colors.py" -NoNewWindow -Wait
Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope_persistence.ui -o ui_spectroscope_persistence.py" -NoNewWindow -Wait
Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope_settings_help.ui -o ui_spectroscope_settings_help.py" -NoNewWindow -Wait
Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope_settings.ui -o ui_spectroscope_settings.py" -NoNewWindow -Wait
Start-Process -FilePath "pyuic6" -ArgumentList "spectroscope_smoothing.ui -o ui_spectroscope_smoothing.py" -NoNewWindow -Wait

Set-Location ..\..\..\

Write-Host "Done!"
# Write-Host "Press any key to exit..."
# Read-Host