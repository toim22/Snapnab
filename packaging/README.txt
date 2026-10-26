Snapnab — screen capture, video recording and image editor
===========================================================

REQUIREMENTS
  Windows 10 version 2004 or later, or Windows 11 (64-bit).
  Nothing else needs to be installed besides this folder.

HOW TO USE
  1. Open Snapnab.exe. It lives in the notification area (next to the clock).
  2. Print Screen          captures an image
     Shift+Print Screen    records a video
     Shift+F9 / Shift+F10  pauses / stops the recording
  3. The capture opens in Snapnab Editor (SnapnabEditor.exe), where you can
     annotate, apply effects, cut videos and save as PNG, JPG, GIF, PDF etc.

  The complete user manual is help\Snapnab-Manual.html (Help > Snapnab Manual,
  or F1, in both programs).

  The files Snapnab.exe, SnapnabEditor.exe and SnapEx64.dll and the lang
  (translations) and help (manual) folders must stay together in the same folder. To install
  with Start menu shortcuts, use Snapnab-<version>-setup.exe.

WHERE YOUR DATA IS STORED
  Captures and videos:  %LOCALAPPDATA%\Snapnab\Library
  Presets:              %LOCALAPPDATA%\Snapnab\Presets1.xml
  Logs:                 %LOCALAPPDATA%\Snapnab\Logs
  Settings:             HKEY_CURRENT_USER\Software\Snapnab

COMMAND LINE
  Snapnab.exe /capture | /video | /preset "name" | /clipboard | /hide | /exit | /help
  SnapnabEditor.exe /Open "file" | /NewFromClipboard | /help

LICENSE
  Copyright © 2026 Antonio Rezende.
  Snapnab is free software, distributed under the GNU General Public License
  version 3 (LICENSE file). No warranty of any kind.

UNINSTALL
  1. Right-click the tray icon and choose "Exit".
  2. If you turned on "Start with Windows" in Options, turn it off before exiting.
  3. Delete this folder. To also delete your captures and settings,
     delete %LOCALAPPDATA%\Snapnab and the HKEY_CURRENT_USER\Software\Snapnab key.
