param(
    [string]$ProcessName,
    [int]$TargetProcessId,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class NativeWindowCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@

$process = if ($TargetProcessId) {
    Get-Process -Id $TargetProcessId -ErrorAction Stop
} elseif ($ProcessName) {
    Get-Process -Name $ProcessName -ErrorAction Stop | Select-Object -First 1
} else {
    throw "ProcessName or TargetProcessId is required."
}
$handle = $process.MainWindowHandle
if ($handle -eq [IntPtr]::Zero) {
    throw "Process '$ProcessName' has no main window."
}

[NativeWindowCapture]::SetForegroundWindow($handle) | Out-Null
Start-Sleep -Milliseconds 300

$rect = New-Object NativeWindowCapture+RECT
if (-not [NativeWindowCapture]::GetWindowRect($handle, [ref]$rect)) {
    throw "Unable to read the window bounds."
}

$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
$bitmap = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $directory = Split-Path -Parent $OutputPath
    if ($directory) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }
    $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
