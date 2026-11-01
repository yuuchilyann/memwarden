<#
Records each app's per-minute private-memory floor for tuning growth.rs (PLAN.md 9.3).

Every 15 s (one sample costs about 0.5 s of WMI work) it reads Win32_Process (name, PID, parent PID, creation date, private bytes only;
never CommandLine), attributes msedgewebview2.exe to its first non-WebView2 ancestor,
and sums private bytes per app name. Once a minute it appends, per app, the lowest sum
seen during that minute, the process count and the earliest process start (to spot
restarts). Output stays in reports\ (gitignored: it lists this machine's process names).
Analyze it with scripts\replay-growth.py. Stop it with Stop-Process.

.EXAMPLE
Start-Process pwsh -ArgumentList '-NoProfile', '-File', 'scripts\growth-logger.ps1' -WindowStyle Hidden -PassThru
#>
param(
    [string]$Out = (Join-Path (Split-Path $PSScriptRoot -Parent) 'reports\growth-log.csv'),
    [int]$IntervalSeconds = 15
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force (Split-Path $Out -Parent) | Out-Null
if (-not (Test-Path $Out)) {
    Set-Content -Path $Out -Value 'minute,app,private_bytes,procs,first_start' -Encoding utf8
}

function Get-Sample {
    $procs = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, PrivatePageCount, CreationDate
    $byPid = @{}
    foreach ($p in $procs) { $byPid[[int]$p.ProcessId] = $p }

    $apps = @{}
    foreach ($p in $procs) {
        if ($p.ProcessId -eq 0) { continue }
        $owner = $p
        $hops = 0
        while ($owner.Name -ieq 'msedgewebview2.exe' -and $hops -lt 16) {
            $parent = $byPid[[int]$owner.ParentProcessId]
            # A parent created after its child means the PID was reused.
            if ($null -eq $parent -or ($parent.CreationDate -and $owner.CreationDate -and $parent.CreationDate -gt $owner.CreationDate)) { break }
            $owner = $parent
            $hops++
        }
        $name = $owner.Name.ToLowerInvariant()
        if (-not $apps.ContainsKey($name)) {
            $apps[$name] = [pscustomobject]@{ Bytes = [uint64]0; Procs = 0; First = [datetime]::MaxValue }
        }
        $a = $apps[$name]
        $a.Bytes += [uint64]$p.PrivatePageCount
        $a.Procs++
        if ($p.CreationDate -and $p.CreationDate -lt $a.First) { $a.First = $p.CreationDate }
    }
    $apps
}

$minute = $null
$floor = @{}
while ($true) {
    $now = Get-Date
    $key = $now.ToString('yyyy-MM-ddTHH:mm')
    if ($minute -and $key -ne $minute) {
        $lines = foreach ($name in $floor.Keys) {
            $f = $floor[$name]
            $first = if ($f.First -eq [datetime]::MaxValue) { '' } else { $f.First.ToString('yyyy-MM-ddTHH:mm:ss') }
            "$minute,$name,$($f.Bytes),$($f.Procs),$first"
        }
        Add-Content -Path $Out -Value $lines -Encoding utf8
        $floor = @{}
    }
    $minute = $key

    try {
        $sample = Get-Sample
        foreach ($name in $sample.Keys) {
            $s = $sample[$name]
            if (-not $floor.ContainsKey($name) -or $s.Bytes -lt $floor[$name].Bytes) { $floor[$name] = $s }
        }
    }
    catch {
        # A failed sample just leaves a gap for this tick.
    }
    Start-Sleep -Seconds ([math]::Max(1, $IntervalSeconds - ((Get-Date) - $now).TotalSeconds))
}
