param(
    [string]$ProjectRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = "Stop"

$project = (Resolve-Path $ProjectRoot).Path
$executable = Join-Path $project "src-tauri\target\release\liteexif.exe"
$qaRoot = Join-Path $project "src-tauri\target\qa\gui-e2e"
$inputDir = Join-Path $qaRoot "input"
$outputDir = Join-Path $qaRoot "output"
$expectedOutput = Join-Path $outputDir "sample.jpeg"
$runtimeConfig = Join-Path $env:APPDATA "com.liteexif.desktop\runtime-rust\config\config.ini"
$configBackup = Join-Path $qaRoot "config.ini.backup"
$stdout = Join-Path $qaRoot "stdout.log"
$stderr = Join-Path $qaRoot "stderr.log"
$screenshot = Join-Path $project "src-tauri\target\qa\liteexif-e2e-complete.png"

New-Item -ItemType Directory -Force -Path $inputDir, $outputDir | Out-Null
Copy-Item -LiteralPath (Join-Path $project "static\normal1.jpeg") -Destination (Join-Path $inputDir "sample.jpeg") -Force
if (Test-Path -LiteralPath $expectedOutput) {
    Remove-Item -LiteralPath $expectedOutput -Force
}
if (Test-Path -LiteralPath $runtimeConfig) {
    Copy-Item -LiteralPath $runtimeConfig -Destination $configBackup -Force
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class E2EGui {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern void SwitchToThisWindow(IntPtr hWnd, bool activate);

    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(
        IntPtr hWnd,
        IntPtr hWndInsertAfter,
        int x,
        int y,
        int width,
        int height,
        uint flags
    );

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
"@

function Invoke-Click {
    param([IntPtr]$Handle, [int]$X, [int]$Y)

    $rect = New-Object E2EGui+RECT
    [E2EGui]::SetWindowPos($Handle, [IntPtr](-1), 0, 0, 0, 0, 0x0043) | Out-Null
    [E2EGui]::SwitchToThisWindow($Handle, $true)
    [E2EGui]::GetWindowRect($Handle, [ref]$rect) | Out-Null
    [E2EGui]::SetForegroundWindow($Handle) | Out-Null
    [E2EGui]::SetCursorPos($rect.Left + $X, $rect.Top + $Y) | Out-Null
    [E2EGui]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [E2EGui]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 350
}

function Set-FieldValue {
    param([IntPtr]$Handle, [int]$X, [int]$Y, [string]$Value)

    Invoke-Click -Handle $Handle -X $X -Y $Y
    [System.Windows.Forms.SendKeys]::SendWait("^a")
    [System.Windows.Forms.Clipboard]::SetText($Value)
    [System.Windows.Forms.SendKeys]::SendWait("^v")
    Start-Sleep -Milliseconds 300
}

$process = $null
try {
    Get-Process liteexif -ErrorAction SilentlyContinue | Stop-Process
    Start-Sleep -Milliseconds 500

    $process = Start-Process -FilePath $executable -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    for ($attempt = 0; $attempt -lt 40 -and $process.MainWindowHandle -eq 0; $attempt++) {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
    }
    if ($process.MainWindowHandle -eq 0) {
        throw "LiteExif window did not open."
    }

    Start-Sleep -Seconds 2
    $handle = $process.MainWindowHandle
    [E2EGui]::SetWindowPos($handle, [IntPtr](-1), 0, 0, 0, 0, 0x0043) | Out-Null
    [E2EGui]::SwitchToThisWindow($handle, $true)

    Invoke-Click -Handle $handle -X 287 -Y 106
    Set-FieldValue -Handle $handle -X 130 -Y 185 -Value $inputDir
    Set-FieldValue -Handle $handle -X 130 -Y 256 -Value $outputDir
    Invoke-Click -Handle $handle -X 254 -Y 892
    Invoke-Click -Handle $handle -X 254 -Y 892
    Start-Sleep -Seconds 3

    $savedConfig = Get-Content -LiteralPath $runtimeConfig -Raw
    if (-not $savedConfig.Contains($inputDir) -or -not $savedConfig.Contains($outputDir)) {
        & (Join-Path $project "scripts\capture-window.ps1") -TargetProcessId $process.Id -OutputPath (Join-Path $project "src-tauri\target\qa\liteexif-save-failed.png")
        Copy-Item -LiteralPath $runtimeConfig -Destination (Join-Path $qaRoot "saved-config-observed.ini") -Force
        throw "The GUI did not persist the test directories."
    }

    Invoke-Click -Handle $handle -X 357 -Y 225
    Invoke-Click -Handle $handle -X 1060 -Y 892

    $created = $false
    for ($attempt = 0; $attempt -lt 90; $attempt++) {
        if (Test-Path -LiteralPath $expectedOutput) {
            $created = $true
            break
        }
        $process.Refresh()
        if ($process.HasExited) {
            break
        }
        Start-Sleep -Seconds 1
    }

    Start-Sleep -Seconds 2
    & (Join-Path $project "scripts\capture-window.ps1") -TargetProcessId $process.Id -OutputPath $screenshot

    if (-not $created) {
        throw "The GUI processing run did not create the expected output."
    }

    [PSCustomObject]@{
        ProcessRunning = -not $process.HasExited
        ConfigSaved = $true
        OutputCreated = $true
        OutputPath = $expectedOutput
        OutputBytes = (Get-Item -LiteralPath $expectedOutput).Length
        Screenshot = $screenshot
        StandardError = (Get-Content -LiteralPath $stderr -Raw -ErrorAction SilentlyContinue)
    }
} finally {
    if ($process -and -not $process.HasExited) {
        $process.CloseMainWindow() | Out-Null
        Start-Sleep -Seconds 1
        $process.Refresh()
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id
        }
    }
    if (Test-Path -LiteralPath $configBackup) {
        Copy-Item -LiteralPath $configBackup -Destination $runtimeConfig -Force
    }
}
