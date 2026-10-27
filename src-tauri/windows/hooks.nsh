; ProxyPilot installer hooks.
;
; ProxyPilot and sing-box run as administrator, so ordinary programs must not
; be able to change the install folder. The installer only inherits the
; parent's permissions, which is safe under Program Files but not on a second
; drive, where signed-in users can usually modify anything. So we give the
; folder its own permissions: SYSTEM and Administrators full control, Users
; read and execute. Groups are given by SID so this works in any language.

!define PP_ICACLS_EXE "$SYSDIR\icacls.exe"
!define PP_GRANTS "*S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F *S-1-5-32-545:(OI)(CI)RX"
!define PP_ADMINS "*S-1-5-32-544"
!define PP_TASK "ProxyPilot Desktop"

; Pushes 1 if the path is a junction or symbolic link, else 0.
!macro PP_IS_LINK pp_path
  Push $R9
  System::Call 'kernel32::GetFileAttributesW(w "${pp_path}") i .R9'
  ${If} $R9 <> -1
    IntOp $R9 $R9 & 0x400
  ${Else}
    StrCpy $R9 0
  ${EndIf}
  ${If} $R9 <> 0
    StrCpy $R9 1
  ${EndIf}
  Exch $R9
!macroend

; Runs icacls and adds 1 to $R8 if it fails. /L acts on links themselves
; rather than on what they point to.
!macro PP_ICACLS pp_args
  nsExec::Exec '"${PP_ICACLS_EXE}" ${pp_args}'
  Exch $R7
  ${If} $R7 != "0"
    IntOp $R8 $R8 + 1
  ${EndIf}
  Pop $R7
!macroend

; sing-box's files, deleted by name: a recursive delete could follow a link
; planted while the folder was still writable.
!macro PP_CLEAN_RUNTIME
  !insertmacro PP_IS_LINK "$INSTDIR\runtime"
  Exch $R9
  ${If} $R9 = 0
    Delete "$INSTDIR\runtime\sing-box.json"
    Delete "$INSTDIR\runtime\sing-box.json.next"
    Delete "$INSTDIR\runtime\sing-box.json.rollback"
    Delete "$INSTDIR\runtime\sing-box.json.tmp"
    Delete "$INSTDIR\runtime\sing-box.candidate.json"
    Delete "$INSTDIR\runtime\sing-box.candidate.json.tmp"
    Delete "$INSTDIR\runtime\sing-box.previous.json"
    Delete "$INSTDIR\runtime\sing-box.log"
    Delete "$INSTDIR\runtime\sing-box.log.1"
    Delete "$INSTDIR\runtime\sing-box.stderr.log"
    Delete "$INSTDIR\runtime\sing-box.stderr.log.1"
    Delete "$INSTDIR\runtime\autostart-task.xml"
  ${EndIf}
  ; Removes a link itself, or the folder once it is empty.
  RMDir "$INSTDIR\runtime"
  Pop $R9
!macroend

; $LANGUAGE 2052 is Simplified Chinese.
!macro NSIS_HOOK_PREINSTALL
  Push $R7
  Push $R8
  StrCpy $R8 0
  !insertmacro PP_IS_LINK "$INSTDIR"
  Pop $R7
  ${If} $R7 = 1
    StrCpy $R8 1
  ${Else}
    ; Take ownership first: an owner can always rewrite permissions.
    !insertmacro PP_ICACLS '"$INSTDIR" /setowner ${PP_ADMINS} /L /Q'
    !insertmacro PP_ICACLS '"$INSTDIR" /reset /L /Q'
    !insertmacro PP_ICACLS '"$INSTDIR" /inheritance:r /grant:r ${PP_GRANTS} /L /Q'
  ${EndIf}
  ${If} $R8 <> 0
    ${If} $LANGUAGE = 2052
      MessageBox MB_ICONSTOP|MB_OK "无法保护安装目录 $INSTDIR 的权限，安装已停止。$\r$\n请换一个安装位置，例如默认的 Program Files。" /SD IDOK
    ${Else}
      MessageBox MB_ICONSTOP|MB_OK "Could not protect the permissions of $INSTDIR, so installation stopped.$\r$\nPlease choose another folder, such as the default under Program Files." /SD IDOK
    ${EndIf}
    Pop $R8
    Pop $R7
    Abort
  ${EndIf}
  Pop $R8
  Pop $R7
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $R7
  Push $R8
  StrCpy $R8 0
  !insertmacro PP_CLEAN_RUNTIME
  CreateDirectory "$INSTDIR\runtime"
  ; Files kept from an older install keep their old permissions; make every
  ; item inherit the folder's again, without recursing into links.
  !insertmacro PP_ICACLS '"$INSTDIR\*" /setowner ${PP_ADMINS} /L /Q'
  !insertmacro PP_ICACLS '"$INSTDIR\*" /reset /L /Q'
  !insertmacro PP_ICACLS '"$INSTDIR\licenses\*" /setowner ${PP_ADMINS} /L /Q'
  !insertmacro PP_ICACLS '"$INSTDIR\licenses\*" /reset /L /Q'
  !insertmacro PP_ICACLS '"$INSTDIR\runtime\*" /setowner ${PP_ADMINS} /L /Q'
  !insertmacro PP_ICACLS '"$INSTDIR\runtime\*" /reset /L /Q'
  ${If} $R8 <> 0
    ${If} $LANGUAGE = 2052
      MessageBox MB_ICONEXCLAMATION|MB_OK "部分文件的权限没能修正。ProxyPilot 启动时会再检查一遍，不安全时会停用分流并提示。" /SD IDOK
    ${Else}
      MessageBox MB_ICONEXCLAMATION|MB_OK "Some file permissions could not be fixed. ProxyPilot checks again at startup and will not route traffic if they are unsafe." /SD IDOK
    ${EndIf}
  ${EndIf}
  Pop $R8
  Pop $R7
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro PP_CLEAN_RUNTIME
  RMDir "$INSTDIR"
  ; The logon task runs ProxyPilot elevated; left behind, it would run
  ; whatever someone later puts at this path. Updates keep it.
  ${If} $UpdateMode <> 1
    Push $R9
    nsExec::Exec '"$SYSDIR\schtasks.exe" /Delete /TN "${PP_TASK}" /F'
    Pop $R9
    Pop $R9
  ${EndIf}
!macroend
