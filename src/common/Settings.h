// Backup and transfer of the settings (F51): one .snapsettings file with the registry values
// (HKCU\Software\Snapnab\1 and its subkeys: options, language, toolbar layout…) and the settings files of the data
// folder (presets, tool themes and quick styles, favorites, tool styles, quick effects, stamp order)
#pragma once
#include "Common.h"

namespace snap::settings {

bool Export(const std::wstring& path, std::wstring& err);
// replace: the current settings give way to the file's; merge: the current ones stay and the file adds what is missing
// (registry values and list items — presets, themes, favorites, quick effects — by name)
bool Import(const std::wstring& path, bool merge, std::wstring& err);
bool Validate(const std::wstring& path);             // a settings file that can be imported
// merge of one settings file (XML): the current items stay and the imported ones that are missing are added
std::string MergeXml(const std::string& current, const std::string& imported);
std::wstring BackupDir();                            // %LOCALAPPDATA%\Snapnab\Backups
// automatic backup once a week (keeps the last 5); returns the file written or an empty string
std::wstring AutoBackup();

} // namespace snap::settings
