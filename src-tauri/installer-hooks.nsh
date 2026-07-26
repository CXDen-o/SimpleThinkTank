; SimpleThinkTank NSIS 安装器钩子
; 通过 tauri.conf.json bundle.windows.nsis.installerHooks 注入官方模板:
;   1) 安装/卸载完成后隐藏进度条(保留"已完成"状态与日志)
;   2) 安装完成后检测 Ollama,未安装则引导下载(可跳过,应用内参数页也可安装)

; 隐藏安装/卸载进度页(instfiles)的进度条控件(标准对话框控件 ID 1004)
!macro HideInstFilesProgressBar
  FindWindow $R8 "#32770" "" $HWNDPARENT
  ${If} $R8 <> 0
    GetDlgItem $R9 $R8 1004
    ShowWindow $R9 ${SW_HIDE}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro HideInstFilesProgressBar

  ; Ollama 引导:静默(/S)与被动(/P)模式下不弹窗
  ${If} $PassiveMode <> 1
  ${AndIfNot} ${Silent}
    ; 检测常见安装位置(每用户 / 每机器)
    StrCpy $R7 0
    ${If} ${FileExists} "$LOCALAPPDATA\Programs\Ollama\ollama.exe"
      StrCpy $R7 1
    ${EndIf}
    ${If} ${FileExists} "$PROGRAMFILES64\Ollama\ollama.exe"
      StrCpy $R7 1
    ${EndIf}

    ${If} $R7 = 0
      ; 按安装语言选择提示文本(避免在语言表加载前定义 LangString)
      StrCpy $R6 "SimpleThinkTank requires Ollama to run local AI models, but it was not detected on this system.$\r$\n$\r$\nClick Yes to open the Ollama download page, or No to skip (you can also install it later from the app's Settings page)."
      StrCmp $LANGUAGE ${LANG_SIMPCHINESE} 0 sttOllamaPrompt
      StrCpy $R6 "智识库需要 Ollama 提供本地大模型能力,检测到当前系统尚未安装。$\r$\n$\r$\n点击「是」打开 Ollama 官网下载页;点击「否」暂不下载(之后也可在应用「参数」页一键安装)。"
      sttOllamaPrompt:
      MessageBox MB_YESNO|MB_ICONQUESTION $R6 /SD IDNO IDYES sttDownloadOllama
      Goto sttSkipOllama
      sttDownloadOllama:
        ExecShell "open" "https://ollama.com/download/windows"
      sttSkipOllama:
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro HideInstFilesProgressBar
!macroend
