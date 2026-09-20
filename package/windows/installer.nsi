;===========================================================
; AutoSign installer (64-bit)
;
; Differences from the upstream Impactor installer:
;   * Simplified Chinese UI (English fallback)
;   * installs the Apple device drivers / Bonjour when they are missing
;     so that a freshly installed Windows PC can sign and install right away
;   * optional launch-on-startup (needed for automatic renewals while the
;     phone is on the same network)
;===========================================================

Unicode true

!define APPNAME "AutoSign"
!define APPNAME_CN "自动签名助手"
!define APPEXE  "AutoSign.exe"
!define COMPANY "AutoSign"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
; Must match the value name the application itself uses (crate::APP_NAME) so the
; "Launch on Startup" checkbox inside AutoSign stays in sync with the installer.
!define RUN_VALUE "AutoSign"

!ifndef APPVERSION
    !error "APPVERSION must be defined when building the installer"
!endif

Name "${APPNAME_CN} (AutoSign)"
BrandingText "${APPNAME} Setup"

OutFile "installer.exe"
RequestExecutionLevel admin
InstallDir "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "${UNINST_KEY}" "InstallLocation"
ShowInstDetails show
ShowUninstDetails show

;-----------------------------------------------------------
; Modern UI 2
;-----------------------------------------------------------
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"

!define MUI_WELCOMEPAGE_TITLE "$(welcomeTitle)"
!define MUI_WELCOMEPAGE_TEXT "$(welcomeText)"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\${APPEXE}"
!define MUI_FINISHPAGE_RUN_TEXT "$(finishRun)"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"

;-----------------------------------------------------------
; Strings
;-----------------------------------------------------------
LangString welcomeTitle ${LANG_SIMPCHINESE} "欢迎安装 自动签名助手 AutoSign"
LangString welcomeTitle ${LANG_ENGLISH} "Welcome to AutoSign Setup"

LangString welcomeText ${LANG_SIMPCHINESE} "AutoSign 可以用你的 Apple ID 为 iPhone 临时签名并安装 IPA，支持连接数据线或手机与电脑在同一 Wi-Fi 时自动续签。$\r$\n$\r$\n安装程序会自动检查并安装 Apple 手机驱动（Apple Mobile Device Support）与 Bonjour，新电脑装完即可使用。$\r$\n$\r$\n点击“下一步”继续。"
LangString welcomeText ${LANG_ENGLISH} "AutoSign signs and installs IPA files with your Apple ID and automatically renews them over USB or Wi-Fi.$\r$\n$\r$\nSetup also installs the Apple device drivers (Apple Mobile Device Support) and Bonjour if they are missing.$\r$\n$\r$\nClick Next to continue."

LangString secMain ${LANG_SIMPCHINESE} "自动签名助手（必需）"
LangString secMain ${LANG_ENGLISH} "AutoSign (required)"
LangString secMainDesc ${LANG_SIMPCHINESE} "安装 AutoSign 主程序和快捷方式。"
LangString secMainDesc ${LANG_ENGLISH} "Installs the AutoSign application and shortcuts."

LangString secDeps ${LANG_SIMPCHINESE} "Apple 手机驱动与依赖（推荐）"
LangString secDeps ${LANG_ENGLISH} "Apple device drivers and dependencies (recommended)"
LangString secDepsDesc ${LANG_SIMPCHINESE} "检测并安装 Apple Mobile Device Support 与 Bonjour。没有它们，电脑无法识别 iPhone，也无法在 Wi-Fi 下续签。"
LangString secDepsDesc ${LANG_ENGLISH} "Detects and installs Apple Mobile Device Support and Bonjour. Without them Windows cannot talk to the iPhone over USB or Wi-Fi."

LangString secAutoStart ${LANG_SIMPCHINESE} "开机自动启动（推荐）"
LangString secAutoStart ${LANG_ENGLISH} "Start with Windows (recommended)"
LangString secAutoStartDesc ${LANG_SIMPCHINESE} "开机后自动在后台运行，手机连上电脑或处于同一 Wi-Fi 时自动续签。"
LangString secAutoStartDesc ${LANG_ENGLISH} "Runs AutoSign in the background so apps are renewed automatically when the phone is connected or on the same network."

LangString depsChecking ${LANG_SIMPCHINESE} "正在检查 Apple 驱动..."
LangString depsChecking ${LANG_ENGLISH} "Checking Apple device drivers..."
LangString depsInstalling ${LANG_SIMPCHINESE} "正在安装 Apple 驱动与组件（需要联网，可能需要几分钟）..."
LangString depsInstalling ${LANG_ENGLISH} "Installing Apple device drivers (internet connection required, this may take a few minutes)..."
LangString depsOk ${LANG_SIMPCHINESE} "Apple 驱动已就绪。"
LangString depsOk ${LANG_ENGLISH} "Apple device drivers are ready."
LangString depsFailed ${LANG_SIMPCHINESE} "自动安装 Apple 驱动未成功。$\r$\n$\r$\n可以手动安装 iTunes 解决：https://www.apple.com/itunes/download/win64 $\r$\n装完后打开 AutoSign 即可；也可以在 AutoSign 的“设置”里点『安装/修复 Apple 驱动』重试。$\r$\n$\r$\n详细日志（发我排查用）：%TEMP%\autosign-deps.log"
LangString depsFailed ${LANG_ENGLISH} "Automatic driver installation did not succeed.$\r$\n$\r$\nInstall iTunes manually to fix it: https://www.apple.com/itunes/download/win64 $\r$\nYou can also retry from AutoSign (Settings -> Install / repair Apple drivers).$\r$\n$\r$\nLog for troubleshooting: %TEMP%\autosign-deps.log"

LangString finishRun ${LANG_SIMPCHINESE} "立即运行 自动签名助手"
LangString finishRun ${LANG_ENGLISH} "Run AutoSign now"

LangString uninstKeepData ${LANG_SIMPCHINESE} "是否同时删除 AutoSign 的账号与证书数据？选择“否”会保留，方便以后重新安装。"
LangString uninstKeepData ${LANG_ENGLISH} "Do you also want to remove the stored accounts and certificates? Choose No to keep them for a later re-install."

LangString uninstTaskKillFailed ${LANG_SIMPCHINESE} "未能结束正在运行的 AutoSign（可能在免打扰模式下继续运行），部分文件可能无法删除。"
LangString uninstTaskKillFailed ${LANG_ENGLISH} "AutoSign could not be stopped; some files may remain until it is closed."

LangString only64 ${LANG_SIMPCHINESE} "AutoSign 仅支持 64 位 Windows 10 / 11。"
LangString only64 ${LANG_ENGLISH} "AutoSign only supports 64-bit Windows 10 / 11."

;-----------------------------------------------------------
; Checks
;-----------------------------------------------------------
Function .onInit
    ; Always start the wizard in Simplified Chinese (users can still switch on
    ; the first page if a different language is wanted).
    StrCpy $LANGUAGE ${LANG_SIMPCHINESE}

    ${IfNot} ${RunningX64}
        MessageBox MB_ICONSTOP|MB_OK "$(only64)"
        Abort
    ${EndIf}
FunctionEnd

;-----------------------------------------------------------
; Installer sections
;-----------------------------------------------------------
Section "!${APPNAME}" SecMain
    SectionIn RO

    SetOutPath "$INSTDIR"
    File "${APPEXE}"
    File "icon.ico"
    ; Kept next to the application so the user can re-run the driver installer
    ; from inside AutoSign if the automatic step below did not succeed.
    File /oname=install-drivers.ps1 "deps.ps1"
    File "collect-logs.ps1"
    ; Bundled anisette support libraries (Apple Music Android slice). These are
    ; what allow signing to work without depending on a remote anisette server.
    SetOutPath "$INSTDIR\libs"
    File /r "libs\*.*"

    ; Apple's own driver packages, shipped so that a PC without internet access
    ; (or without winget / the Microsoft Store) can still install the drivers.
    SetOutPath "$INSTDIR\drivers"
    File /r "drivers\*.*"
    SetOutPath "$INSTDIR"

    CreateDirectory "$SMPROGRAMS\${APPNAME_CN}"
    CreateShortcut "$SMPROGRAMS\${APPNAME_CN}\${APPNAME_CN}.lnk" "$INSTDIR\${APPEXE}" "" "$INSTDIR\icon.ico"
    CreateShortcut "$SMPROGRAMS\${APPNAME_CN}\收集诊断日志.lnk" "powershell.exe" '-NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\collect-logs.ps1"' "$INSTDIR\icon.ico"
    CreateShortcut "$DESKTOP\${APPNAME_CN}.lnk" "$INSTDIR\${APPEXE}" "" "$INSTDIR\icon.ico"

    WriteUninstaller "$INSTDIR\Uninstall.exe"

    WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "${APPNAME_CN} (${APPNAME})"
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${APPVERSION}"
    WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "${COMPANY}"
    WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\icon.ico"
    WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1

    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "$(secDeps)" SecDeps
    DetailPrint "$(depsChecking)"

    ; $3 = result: 0 = failed, 1 = ready
    StrCpy $3 "0"

    ; Newer Apple installers register "AppleMobileDeviceService", older ones use
    ; "Apple Mobile Device Service" with spaces - accept both.
    ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Services\Apple Mobile Device Service" "ImagePath"
    ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\AppleMobileDeviceService" "ImagePath"
    StrCmp $0 "" 0 deps_already
    StrCmp $1 "" 0 deps_already
    Goto deps_install

    deps_already:
        StrCpy $3 "1"
        Goto deps_report

    deps_install:
        DetailPrint "$(depsInstalling)"
        SetOutPath "$PLUGINSDIR"
        File "deps.ps1"
        SetOutPath "$INSTDIR"

        ; Remove stale markers so an old run cannot be mistaken for this one.
        Delete "$TEMP\autosign-deps-ok.txt"
        Delete "$TEMP\autosign-deps-failed.txt"

        nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -File "$PLUGINSDIR\deps.ps1" -LogPath "$TEMP\autosign-deps.log" -BundleDir "$INSTDIR\drivers"'
        Pop $2

        IfFileExists "$TEMP\autosign-deps-ok.txt" 0 deps_check_failed_marker
            StrCpy $3 "1"
            Goto deps_registry_check

    deps_check_failed_marker:
        IfFileExists "$TEMP\autosign-deps-failed.txt" 0 deps_registry_check
            StrCpy $3 "0"

    deps_registry_check:
        ; Fall back to the registry in case the helper did not get that far.
        ReadRegStr $0 HKLM "SYSTEM\CurrentControlSet\Services\Apple Mobile Device Service" "ImagePath"
        ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\AppleMobileDeviceService" "ImagePath"
        StrCmp $0 "" 0 deps_mark_ok
        StrCmp $1 "" 0 deps_mark_ok
        Goto deps_report

    deps_mark_ok:
        StrCpy $3 "1"

    deps_report:
        StrCmp $3 "1" 0 deps_failed
        DetailPrint "$(depsOk)"
        Goto deps_done

    deps_failed:
        DetailPrint "$(depsFailed)"
        MessageBox MB_ICONEXCLAMATION|MB_OK "$(depsFailed)"

    deps_done:
SectionEnd

Section "$(secAutoStart)" SecAutoStart
    WriteRegStr HKCU "${RUN_KEY}" "${RUN_VALUE}" '"$INSTDIR\${APPEXE}" --tray'
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "$(secMainDesc)"
    !insertmacro MUI_DESCRIPTION_TEXT ${SecDeps} "$(secDepsDesc)"
    !insertmacro MUI_DESCRIPTION_TEXT ${SecAutoStart} "$(secAutoStartDesc)"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

;-----------------------------------------------------------
; Uninstaller
;-----------------------------------------------------------
Section "Uninstall"
    ; Make sure the app is not running while we delete it.
    nsExec::ExecToLog 'taskkill /IM ${APPEXE} /F'

    Delete "$DESKTOP\${APPNAME_CN}.lnk"
    Delete "$SMPROGRAMS\${APPNAME_CN}\${APPNAME_CN}.lnk"
    Delete "$SMPROGRAMS\${APPNAME_CN}\收集诊断日志.lnk"
    RMDir "$SMPROGRAMS\${APPNAME_CN}"

    DeleteRegValue HKCU "${RUN_KEY}" "${RUN_VALUE}"
    DeleteRegKey HKLM "${UNINST_KEY}"

    Delete "$INSTDIR\${APPEXE}"
    Delete "$INSTDIR\icon.ico"
    Delete "$INSTDIR\install-drivers.ps1"
    Delete "$INSTDIR\collect-logs.ps1"
    RMDir /r "$INSTDIR\drivers"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir /r "$INSTDIR\libs"
    RMDir "$INSTDIR"

    MessageBox MB_YESNO|MB_ICONQUESTION "$(uninstKeepData)" IDYES remove_data
    Goto done

    remove_data:
        RMDir /r "$APPDATA\AutoSign"

    done:
SectionEnd
