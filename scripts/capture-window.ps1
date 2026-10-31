<#
.SYNOPSIS
    Launches memwarden once per language and saves a screenshot of its window.
.DESCRIPTION
    Uses PrintWindow so only the memwarden window is captured, never other
    windows on screen. Screenshots contain process names of this machine;
    keep them out of the repository.
.EXAMPLE
    .\scripts\capture-window.ps1 -Exe .\target\debug\memwarden.exe -OutDir .\screenshots -Langs zh-TW,en
#>
param(
    [Parameter(Mandatory)] [string] $Exe,
    [Parameter(Mandatory)] [string] $OutDir,
    [string[]] $Langs = @('en'),
    [int] $WaitSeconds = 8
)

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class MemwardenCapture {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public struct Rect { public int Left, Top, Right, Bottom; }
}
"@
Add-Type -AssemblyName System.Drawing
[MemwardenCapture]::SetProcessDPIAware() | Out-Null
# .NET resolves relative paths against the process directory, not the PowerShell location.
$OutDir = (New-Item -ItemType Directory -Force $OutDir).FullName
$PW_RENDERFULLCONTENT = 2

foreach ($lang in $Langs) {
    $env:MEMWARDEN_LANG = $lang
    $p = Start-Process $Exe -PassThru
    Start-Sleep $WaitSeconds
    $p.Refresh()
    $r = New-Object MemwardenCapture+Rect
    [MemwardenCapture]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
    $bmp = New-Object Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [MemwardenCapture]::PrintWindow($p.MainWindowHandle, $hdc, $PW_RENDERFULLCONTENT) | Out-Null
    $g.ReleaseHdc($hdc)
    $out = Join-Path $OutDir "memwarden-$lang.png"
    $bmp.Save($out)
    $g.Dispose(); $bmp.Dispose()
    Stop-Process -Id $p.Id -Confirm:$false
    Write-Output $out
    Start-Sleep 1
}
Remove-Item Env:MEMWARDEN_LANG
