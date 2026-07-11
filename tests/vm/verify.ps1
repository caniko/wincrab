# wincrab headless verification script
# Runs inside a freshly installed Windows 11 VM and outputs test results to COM1.
#
# Exit behavior: writes WINCRAB_TEST_PASS or WINCRAB_TEST_FAIL to serial,
# then shuts down the VM.

$ErrorActionPreference = "Continue"

# --- Serial port setup -------------------------------------------------------
$port = New-Object System.IO.Ports.SerialPort COM1,115200,None,8,One
$port.Open()

$pass = 0
$fail = 0

function Log($msg) {
    $port.WriteLine($msg)
    Write-Host $msg
}

function Check-Reg($path, $name, $expected, $label) {
    try {
        $val = Get-ItemPropertyValue -Path $path -Name $name -ErrorAction Stop
        if ($val -eq $expected) {
            Log "PASS: $label"
            $script:pass++
        } else {
            Log "FAIL: $label (got $val, want $expected)"
            $script:fail++
        }
    } catch {
        Log "FAIL: $label (key not found: $path\$name)"
        $script:fail++
    }
}

function Check-RegExists($path, $name, $label) {
    try {
        $null = Get-ItemPropertyValue -Path $path -Name $name -ErrorAction Stop
        Log "PASS: $label"
        $script:pass++
    } catch {
        Log "FAIL: $label (key not found: $path\$name)"
        $script:fail++
    }
}

function Check-NoAppx($pattern, $label) {
    $found = Get-AppxPackage -AllUsers -Name $pattern -ErrorAction SilentlyContinue
    if ($found) {
        Log "FAIL: $label still installed"
        $script:fail++
    } else {
        Log "PASS: $label removed"
        $script:pass++
    }
}

function Check-ServiceDisabled($name, $label) {
    $svc = Get-Service -Name $name -ErrorAction SilentlyContinue
    if (-not $svc) {
        Log "PASS: $label (not present)"
        $script:pass++
    } elseif ($svc.StartType -eq 'Disabled') {
        Log "PASS: $label (disabled)"
        $script:pass++
    } else {
        Log "FAIL: $label (StartType=$($svc.StartType))"
        $script:fail++
    }
}

function Check-TaskDisabled($taskPath, $taskName, $label) {
    $task = Get-ScheduledTask -TaskPath $taskPath -TaskName $taskName -ErrorAction SilentlyContinue
    if (-not $task) {
        Log "PASS: $label (not present)"
        $script:pass++
    } elseif ($task.State -eq 'Disabled' -or $task.Settings.Enabled -eq $false) {
        Log "PASS: $label (disabled)"
        $script:pass++
    } else {
        Log "FAIL: $label (State=$($task.State), Enabled=$($task.Settings.Enabled))"
        $script:fail++
    }
}

function Check-FileExists($path, $label) {
    if (Test-Path $path) {
        Log "PASS: $label"
        $script:pass++
    } else {
        Log "FAIL: $label (not found: $path)"
        $script:fail++
    }
}

function Check-FileAbsent($path, $label) {
    if (Test-Path $path) {
        Log "FAIL: $label still present ($path)"
        $script:fail++
    } else {
        Log "PASS: $label removed"
        $script:pass++
    }
}

function Check-AppxPresent($pattern, $label) {
    $found = Get-AppxPackage -AllUsers -Name $pattern -ErrorAction SilentlyContinue
    if ($found) {
        Log "PASS: $label present"
        $script:pass++
    } else {
        Log "FAIL: $label missing"
        $script:fail++
    }
}

function Check-FileContains($path, $needle, $label) {
    if (-not (Test-Path $path)) {
        Log "FAIL: $label (file not found: $path)"
        $script:fail++
        return
    }
    $content = Get-Content $path -Raw -ErrorAction SilentlyContinue
    if ($content -and $content.Contains($needle)) {
        Log "PASS: $label"
        $script:pass++
    } else {
        Log "FAIL: $label (string not found in $path)"
        $script:fail++
    }
}

# === Run pending Active Setup entries (normally run by Explorer at logon) ======
# FirstLogonCommands execute before Explorer starts, so Active Setup entries
# haven't run yet.  Process WinCrab entries now so HKCU values are applied.
$activeSetupRoot = "HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components"
Get-ChildItem $activeSetupRoot -ErrorAction SilentlyContinue | Where-Object {
    $_.PSChildName -like "WinCrab.*"
} | ForEach-Object {
    $stub = (Get-ItemProperty $_.PSPath -Name "StubPath" -ErrorAction SilentlyContinue).StubPath
    if ($stub) {
        try { cmd /c $stub 2>$null } catch {}
    }
}

# === Begin tests ==============================================================
Log ""
Log "WINCRAB_TEST_START"
Log "=============================="

# --- Telemetry ----------------------------------------------------------------
Log ""
Log "[Telemetry]"
# Group Policy path (takes precedence over DataCollection path)
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection" `
    "AllowTelemetry" 0 "AllowTelemetry = 0 (GP)"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection" `
    "MaxTelemetryAllowed" 0 "MaxTelemetryAllowed = 0 (GP)"
Check-Reg "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection" `
    "AllowDeviceNameInTelemetry" 0 "AllowDeviceNameInTelemetry = 0"
Check-Reg "HKLM:\SOFTWARE\Microsoft\SQMClient\Windows" `
    "CEIPEnable" 0 "CEIP disabled"
Check-Reg "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\AppCompat" `
    "AITEnable" 0 "App telemetry disabled"

# --- Telemetry extensions (new) -----------------------------------------------
Log ""
Log "[Telemetry Extensions]"
Check-Reg "HKCU:\Software\Microsoft\Clipboard" `
    "EnableClipboardHistory" 0 "Clipboard sync disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search" `
    "BingSearchEnabled" 0 "Bing search disabled (user)"
Check-Reg "HKCU:\Software\Microsoft\Siuf\Rules" `
    "NumberOfSIUFInPeriod" 0 "Feedback frequency = Never"

# --- Copilot ------------------------------------------------------------------
Log ""
Log "[Copilot]"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot" `
    "TurnOffWindowsCopilot" 1 "Copilot disabled (machine policy)"
Check-Reg "HKCU:\Software\Policies\Microsoft\Windows\WindowsCopilot" `
    "TurnOffWindowsCopilot" 1 "Copilot disabled (user policy)"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "ShowCopilotButton" 0 "Copilot taskbar button hidden"

# --- Privacy ------------------------------------------------------------------
Log ""
Log "[Privacy]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo" `
    "Enabled" 0 "Advertising ID disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\Explorer" `
    "DisableSearchBoxSuggestions" 1 "Web search disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" `
    "EnableActivityFeed" 0 "Activity feed disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" `
    "PublishUserActivities" 0 "Activity history disabled"
Check-Reg "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting" `
    "Disabled" 1 "Error reporting disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Privacy" `
    "TailoredExperiencesWithDiagnosticDataEnabled" 0 "Tailored experiences disabled"
# App permissions (capability access manager defaults)
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location" `
    "Value" "Deny" "Location access default = Deny"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam" `
    "Value" "Deny" "Webcam access default = Deny"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone" `
    "Value" "Deny" "Microphone access default = Deny"

# --- Privacy extensions (new) -------------------------------------------------
Log ""
Log "[Privacy Extensions]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" `
    "SubscribedContent-310093Enabled" 0 "Suggested actions disabled (CDM)"

# --- Edge policies ------------------------------------------------------------
Log ""
Log "[Edge]"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Edge" `
    "HideFirstRunExperience" 1 "Edge first-run hidden"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Edge" `
    "DefaultBrowserSettingEnabled" 0 "Edge default browser nag disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Edge" `
    "HubsSidebarEnabled" 0 "Edge sidebar disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Edge" `
    "SearchbarAllowed" 0 "Edge search bar disabled"

# --- Visuals ------------------------------------------------------------------
Log ""
Log "[Visuals]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects" `
    "VisualFXSetting" 2 "Visual effects = performance"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" `
    "EnableTransparency" 0 "Transparency disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\DWM" `
    "EnableAeroPeek" 0 "Aero Peek disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "TaskbarAnimations" 0 "Taskbar animations disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "ListviewAlphaSelect" 0 "Listview alpha select disabled"
# ContentDeliveryManager — lock screen tips + suggestions
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" `
    "RotatingLockScreenOverlayEnabled" 0 "Lock screen tips disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" `
    "SystemPaneSuggestionsEnabled" 0 "Start suggestions disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" `
    "SubscribedContent-338388Enabled" 0 "Suggestion notifications disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" `
    "SubscribedContent-338389Enabled" 0 "Start recommendations (CDM) disabled"

# --- Dark mode (new) ----------------------------------------------------------
Log ""
Log "[Dark Mode]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" `
    "AppsUseLightTheme" 0 "Apps dark mode enabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" `
    "SystemUsesLightTheme" 0 "System dark mode enabled"

# --- Taskbar ------------------------------------------------------------------
Log ""
Log "[Taskbar]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "TaskbarDa" 0 "Widgets button hidden"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "TaskbarMn" 0 "Chat button hidden"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "SearchboxTaskbarMode" 1 "Search = icon only"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "Start_IrisRecommendations" 0 "Start recommendations disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "TaskbarAl" 0 "Taskbar left-aligned"

# --- Explorer (new) -----------------------------------------------------------
Log ""
Log "[Explorer]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "HideFileExt" 0 "File extensions visible"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced" `
    "LaunchTo" 1 "Explorer launches to This PC"

# --- OOBE nag suppression -----------------------------------------------------
Log ""
Log "[OOBE]"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement" `
    "ScoobeSystemSettingEnabled" 0 "Finish-setup nag suppressed"

# --- Windows Defender (new) ---------------------------------------------------
Log ""
Log "[Defender]"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender" `
    "DisableAntiSpyware" 1 "Defender anti-spyware disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Real-Time Protection" `
    "DisableRealtimeMonitoring" 1 "Defender real-time monitoring disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Real-Time Protection" `
    "DisableBehaviorMonitoring" 1 "Defender behavior monitoring disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet" `
    "SpynetReporting" 0 "Cloud protection disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows Defender\Spynet" `
    "SubmitSamplesConsent" 2 "Sample submission disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" `
    "EnableSmartScreen" 0 "SmartScreen disabled"
Check-ServiceDisabled "WinDefend" "WinDefend (Defender service)"
Check-ServiceDisabled "WdNisSvc" "WdNisSvc (Network Inspection)"

# --- Windows Update (new) ----------------------------------------------------
Log ""
Log "[Windows Update]"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization" `
    "DODownloadMode" 0 "Delivery Optimization disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU" `
    "NoAutoRebootWithLoggedOnUsers" 1 "No auto-reboot with logged on users"

# --- Windows Recall / AI (new) -----------------------------------------------
Log ""
Log "[Recall]"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI" `
    "DisableAIDataAnalysis" 1 "Recall AI data analysis disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI" `
    "TurnOffSavingSnapshots" 1 "Recall snapshots disabled"

# --- Performance tuning (new) ------------------------------------------------
Log ""
Log "[Performance]"
# Accept both 1 (user-managed disabled) and 0x80000002 (system-managed disabled)
$ntfsVal = (Get-ItemPropertyValue -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" `
    -Name "NtfsDisableLastAccessUpdate" -ErrorAction SilentlyContinue)
if ($ntfsVal -eq 1 -or $ntfsVal -eq 0x80000002) {
    Log "PASS: NTFS last access disabled"
    $pass++
} elseif ($null -eq $ntfsVal) {
    Log "FAIL: NTFS last access disabled (key not found)"
    $fail++
} else {
    Log "FAIL: NTFS last access disabled (got $ntfsVal, want 1 or 0x80000002)"
    $fail++
}
Check-Reg "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" `
    "NtfsDisable8dot3NameCreation" 1 "NTFS 8.3 names disabled"
Check-Reg "HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR" `
    "AllowGameDVR" 0 "Game DVR disabled"
Check-Reg "HKCU:\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications" `
    "GlobalUserDisabled" 1 "Background apps disabled"

# --- Security hardening (new) ------------------------------------------------
Log ""
Log "[Security]"
Check-Reg "HKLM:\SYSTEM\CurrentControlSet\Control\Terminal Server" `
    "fDenyTSConnections" 1 "Remote Desktop disabled"
Check-ServiceDisabled "mrxsmb10" "SMBv1 client (disabled)"

# --- Services -----------------------------------------------------------------
Log ""
Log "[Services]"
Check-ServiceDisabled "DiagTrack" "DiagTrack (telemetry)"
Check-ServiceDisabled "dmwappushservice" "dmwappush (WAP push)"
Check-ServiceDisabled "WerSvc" "WerSvc (error reporting)"
Check-ServiceDisabled "XblAuthManager" "XblAuthManager (Xbox)"
Check-ServiceDisabled "XblGameSave" "XblGameSave (Xbox)"
Check-ServiceDisabled "XboxNetApiSvc" "XboxNetApiSvc (Xbox)"
Check-ServiceDisabled "MapsBroker" "MapsBroker (maps)"
Check-ServiceDisabled "lfsvc" "lfsvc (geolocation)"
# New services
Check-ServiceDisabled "SSDPSRV" "SSDPSRV (SSDP Discovery)"
Check-ServiceDisabled "upnphost" "upnphost (UPnP Device Host)"
Check-ServiceDisabled "Fax" "Fax service"
Check-ServiceDisabled "WMPNetworkSvc" "WMPNetworkSvc (media sharing)"
Check-ServiceDisabled "WpnService" "WpnService (widgets/push)"
Check-ServiceDisabled "TapiSrv" "TapiSrv (telephony)"

# --- Appx packages ------------------------------------------------------------
Log ""
Log "[Appx Packages]"
Check-NoAppx "*BingNews*" "Bing News"
Check-NoAppx "*BingWeather*" "Bing Weather"
Check-NoAppx "*CandyCrush*" "Candy Crush"
Check-NoAppx "*SpotifyMusic*" "Spotify"
Check-NoAppx "*Clipchamp*" "Clipchamp"
Check-NoAppx "*ZuneMusic*" "Groove Music"
Check-NoAppx "*ZuneVideo*" "Movies & TV"
Check-NoAppx "*MicrosoftSolitaireCollection*" "Solitaire"
Check-NoAppx "*Getstarted*" "Tips (Getstarted)"
Check-NoAppx "*WindowsFeedbackHub*" "Feedback Hub"
Check-NoAppx "*XboxApp*" "Xbox App"
Check-NoAppx "*XboxGameOverlay*" "Xbox Game Overlay"
Check-NoAppx "*XboxGamingOverlay*" "Xbox Gaming Overlay"
Check-NoAppx "*MicrosoftTeams*" "Teams"
Check-NoAppx "*549981C3F5F10*" "Cortana"
Check-NoAppx "*OutlookForWindows*" "New Outlook"
Check-NoAppx "*windowscommunicationsapps*" "Mail & Calendar"
Check-NoAppx "*DevHome*" "Dev Home"
Check-NoAppx "*YourPhone*" "Phone Link"
Check-NoAppx "*CrossDevice*" "Cross Device"

# --- Seelen-UI (conditional — only tested when bundle was enabled) ------------
Log ""
Log "[Seelen-UI]"
if (Test-Path "C:\SeelenUI\install.ps1") {
    Log "PASS: Seelen install script injected"
    $pass++
    Check-FileExists "C:\SeelenUI\Seelen.UI-setup.exe" "Seelen UI setup executable present"
    Check-Reg "HKLM:\SOFTWARE\Microsoft\Active Setup\Installed Components\WinCrab.SeelenUI" `
        "StubPath" "powershell.exe -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\SeelenUI\install.ps1" `
        "Seelen Active Setup registered"
} else {
    Log "PASS: Seelen not bundled (skipped)"
    $pass++
}

# --- Edge (must be present — Seelen depends on WebView2) ----------------------
Log ""
Log "[Edge / WebView2]"
Check-AppxPresent "*MicrosoftEdge*" "Edge browser (needed for WebView2)"

# --- OneDrive removal ---------------------------------------------------------
Log ""
Log "[OneDrive]"
Check-FileAbsent "C:\Windows\System32\OneDriveSetup.exe" "OneDrive installer (System32)"
Check-FileAbsent "C:\Windows\SysWOW64\OneDriveSetup.exe" "OneDrive installer (SysWOW64)"

# --- Microsoft Store (should still be present) --------------------------------
Log ""
Log "[Store]"
Check-AppxPresent "*WindowsStore*" "Microsoft Store (kept)"

# --- Scheduled tasks ----------------------------------------------------------
Log ""
Log "[Scheduled Tasks]"
Check-TaskDisabled "\Microsoft\Windows\Application Experience\" "Microsoft Compatibility Appraiser" "Compat Appraiser"
Check-TaskDisabled "\Microsoft\Windows\Application Experience\" "ProgramDataUpdater" "ProgramDataUpdater"
Check-TaskDisabled "\Microsoft\Windows\Customer Experience Improvement Program\" "Consolidator" "CEIP Consolidator"
Check-TaskDisabled "\Microsoft\Windows\Customer Experience Improvement Program\" "UsbCeip" "CEIP UsbCeip"
Check-TaskDisabled "\Microsoft\Windows\DiskDiagnostic\" "Microsoft-Windows-DiskDiagnosticDataCollector" "DiskDiagnostic collector"

# --- Telemetry hosts file (new) -----------------------------------------------
Log ""
Log "[Hosts File]"
Check-FileContains "C:\Windows\System32\drivers\etc\hosts" `
    "0.0.0.0 telemetry.microsoft.com" "Telemetry hosts: telemetry.microsoft.com blocked"
Check-FileContains "C:\Windows\System32\drivers\etc\hosts" `
    "0.0.0.0 vortex.data.microsoft.com" "Telemetry hosts: vortex.data.microsoft.com blocked"
Check-FileContains "C:\Windows\System32\drivers\etc\hosts" `
    "0.0.0.0 watson.telemetry.microsoft.com" "Telemetry hosts: watson blocked"
Check-FileContains "C:\Windows\System32\drivers\etc\hosts" `
    "0.0.0.0 feedback.windows.com" "Telemetry hosts: feedback.windows.com blocked"
Check-FileContains "C:\Windows\System32\drivers\etc\hosts" `
    "wincrab telemetry block" "Telemetry hosts: wincrab marker present"

# --- Performance script (new) ------------------------------------------------
Log ""
Log "[Performance Script]"
Check-FileExists "C:\wincrab\performance.ps1" "Performance script injected"
Check-FileContains "C:\wincrab\performance.ps1" `
    "powercfg" "Performance script contains powercfg"

# === Results ==================================================================
Log ""
Log "=============================="
Log "WINCRAB_RESULTS: $pass passed, $fail failed"
Log ""
if ($fail -eq 0) {
    Log "WINCRAB_TEST_PASS"
} else {
    Log "WINCRAB_TEST_FAIL"
}

$port.Close()

# Shut down the VM
Start-Sleep -Seconds 2
Stop-Computer -Force
