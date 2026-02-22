#
# RustedClaw Benchmark Script — Windows (PowerShell 5.1+)
#
# Usage:
#   .\scripts\benchmark.ps1 [-Binary "path\to\rustedclaw.exe"]
#
# If no path is given, it looks for .\target\release\rustedclaw.exe
#

param(
    [string]$Binary = ".\target\release\rustedclaw.exe",
    [int]$Port = 42617,
    [int]$Requests = 200
)

$ErrorActionPreference = "Stop"
$ResultsFile = "benchmark-results.txt"

function Write-Header($text) { Write-Host "`n=== $text ===" -ForegroundColor Cyan }
function Write-Metric($name, $value) { Write-Host "  ✓ ${name}: $value" -ForegroundColor Green }
function Write-Fail($text) { Write-Host "  ✗ $text" -ForegroundColor Red }

Write-Host ""
Write-Host "  ┌──────────────────────────────────────────┐" -ForegroundColor Cyan
Write-Host "  │   🦀  RustedClaw Benchmark Suite          │" -ForegroundColor Cyan
Write-Host "  │       Windows (PowerShell)                │" -ForegroundColor Cyan
Write-Host "  └──────────────────────────────────────────┘" -ForegroundColor Cyan
Write-Host ""

# ── Check binary ───────────────────────────────────────────────────────────
if (-not (Test-Path $Binary)) {
    Write-Host "Binary not found at: $Binary" -ForegroundColor Red
    Write-Host "Build first: cargo build --release"
    Write-Host "Or specify: .\scripts\benchmark.ps1 -Binary path\to\rustedclaw.exe"
    exit 1
}

$BinaryFullPath = (Resolve-Path $Binary).Path

# ── 1. Binary Size ─────────────────────────────────────────────────────────
Write-Header "Binary Size"
$binInfo = Get-Item $BinaryFullPath
$binSizeMB = [math]::Round($binInfo.Length / 1MB, 2)
Write-Metric "Binary size" "$binSizeMB MB ($($binInfo.Length) bytes)"

# ── 2. Cold Start Time ────────────────────────────────────────────────────
Write-Header "Cold Start Time"
$runs = 10
$totalMs = 0
for ($i = 1; $i -le $runs; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $BinaryFullPath --version 2>&1 | Out-Null
    $sw.Stop()
    $totalMs += $sw.ElapsedMilliseconds
}
$avgMs = [math]::Round($totalMs / $runs, 0)
Write-Metric "Cold start (avg of $runs runs)" "$avgMs ms"

# ── 3. Start Gateway ──────────────────────────────────────────────────────
Write-Header "Starting Gateway (port $Port)"

# Kill any existing instance
Get-Process -Name "rustedclaw" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

$gwProcess = Start-Process -FilePath $BinaryFullPath -ArgumentList "gateway","--port",$Port `
    -PassThru -NoNewWindow -RedirectStandardError "NUL" -RedirectStandardOutput "NUL"
Start-Sleep -Seconds 3

if ($gwProcess.HasExited) {
    Write-Fail "Gateway failed to start"
    exit 1
}
Write-Metric "Gateway PID" $gwProcess.Id

# ── 4. Idle Memory ────────────────────────────────────────────────────────
Write-Header "Idle Memory"
$p = Get-Process -Id $gwProcess.Id
$rssIdleMB = [math]::Round($p.WorkingSet64 / 1MB, 2)
$privateMB = [math]::Round($p.PrivateMemorySize64 / 1MB, 2)
$virtualMB = [math]::Round($p.VirtualMemorySize64 / 1MB, 2)
$cpuIdle = [math]::Round($p.CPU, 3)
$threads = $p.Threads.Count
$handles = $p.HandleCount

Write-Metric "Working Set (RAM)" "$rssIdleMB MB"
Write-Metric "Private Memory" "$privateMB MB"
Write-Metric "Virtual Memory" "$virtualMB MB"
Write-Metric "CPU Time" "$cpuIdle sec"
Write-Metric "Threads" $threads
Write-Metric "Handles" $handles

# ── 5. Load Test ──────────────────────────────────────────────────────────
Write-Header "Load Test ($Requests requests)"
$baseUrl = "http://127.0.0.1:$Port"
$passed = 0
$failed = 0

# Warm up
try { Invoke-WebRequest -Uri "$baseUrl/health" -UseBasicParsing -TimeoutSec 5 | Out-Null } catch {}

$swLoad = [System.Diagnostics.Stopwatch]::StartNew()

for ($i = 1; $i -le $Requests; $i++) {
    try {
        $r = Invoke-WebRequest -Uri "$baseUrl/v1/status" -UseBasicParsing -TimeoutSec 5
        if ($r.StatusCode -eq 200) { $passed++ } else { $failed++ }
    } catch {
        $failed++
    }
}

$swLoad.Stop()
$loadSec = [math]::Round($swLoad.Elapsed.TotalSeconds, 2)
$rps = [math]::Round($Requests / $swLoad.Elapsed.TotalSeconds, 1)

Write-Metric "Requests" "$passed passed, $failed failed"
Write-Metric "Duration" "${loadSec}s"
Write-Metric "Throughput" "$rps req/sec"

# ── 6. Memory After Load ─────────────────────────────────────────────────
Write-Header "Memory After Load"
$pAfter = Get-Process -Id $gwProcess.Id
$rssAfterMB = [math]::Round($pAfter.WorkingSet64 / 1MB, 2)
$privateAfterMB = [math]::Round($pAfter.PrivateMemorySize64 / 1MB, 2)
$cpuAfter = [math]::Round($pAfter.CPU, 3)
$growth = [math]::Round($rssAfterMB - $rssIdleMB, 2)

Write-Metric "Working Set after load" "$rssAfterMB MB"
Write-Metric "Private Memory after load" "$privateAfterMB MB"
Write-Metric "Memory growth" "$growth MB"
Write-Metric "CPU Time after load" "$cpuAfter sec"

# ── 7. API Endpoint Validation ────────────────────────────────────────────
Write-Header "API Endpoint Validation"

$endpoints = @(
    @{Method="GET"; Path="/health"},
    @{Method="GET"; Path="/v1/status"},
    @{Method="GET"; Path="/v1/tools"},
    @{Method="GET"; Path="/v1/conversations"},
    @{Method="GET"; Path="/v1/routines"},
    @{Method="GET"; Path="/v1/memory?q=test"},
    @{Method="GET"; Path="/v1/jobs"},
    @{Method="GET"; Path="/v1/config"},
    @{Method="GET"; Path="/"},
    @{Method="GET"; Path="/static/style.css"},
    @{Method="GET"; Path="/static/app.js"}
)

$epPass = 0
$epFail = 0
foreach ($ep in $endpoints) {
    try {
        $r = Invoke-WebRequest -Uri "$baseUrl$($ep.Path)" -Method $ep.Method -UseBasicParsing -TimeoutSec 5
        if ($r.StatusCode -eq 200) {
            Write-Metric "$($ep.Method) $($ep.Path)" "$($r.StatusCode) OK"
            $epPass++
        } else {
            Write-Fail "$($ep.Method) $($ep.Path) → $($r.StatusCode)"
            $epFail++
        }
    } catch {
        Write-Fail "$($ep.Method) $($ep.Path) → Error"
        $epFail++
    }
}

Write-Host ""
Write-Metric "Endpoints" "$epPass passed, $epFail failed"

# ── Cleanup ───────────────────────────────────────────────────────────────
Write-Header "Cleanup"
Stop-Process -Id $gwProcess.Id -Force -ErrorAction SilentlyContinue
Write-Metric "Gateway stopped" "PID $($gwProcess.Id)"

# ── Summary ───────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "╔══════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║        🦀 RustedClaw Benchmark Summary           ║" -ForegroundColor Cyan
Write-Host "╠══════════════════════════════════════════════════╣" -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Binary Size:", "$binSizeMB MB") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Cold Start:", "$avgMs ms") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Idle RAM:", "$rssIdleMB MB") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Private Memory:", "$privateMB MB") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "RAM After Load:", "$rssAfterMB MB") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Memory Growth:", "$growth MB") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Throughput:", "$rps req/sec") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Threads:", "$threads") -ForegroundColor Cyan
Write-Host ("║  {0,-22} {1,24} ║" -f "Endpoints:", "$epPass/$($epPass + $epFail) OK") -ForegroundColor Cyan
Write-Host "╚══════════════════════════════════════════════════╝" -ForegroundColor Cyan

# Save results
@"
RustedClaw Benchmark Results
$(Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC")
$([System.Environment]::OSVersion.VersionString) — $([System.Environment]::ProcessorCount) CPUs

Binary Size:      $binSizeMB MB
Cold Start:       $avgMs ms (avg of $runs runs)
Idle RAM (WS):    $rssIdleMB MB
Private Memory:   $privateMB MB
RAM After Load:   $rssAfterMB MB
Memory Growth:    $growth MB
Load Test:        $Requests requests in ${loadSec}s ($rps req/s)
Throughput:       $rps req/sec
Threads:          $threads
Handles:          $handles
Endpoints:        $epPass/$($epPass + $epFail) OK
"@ | Set-Content -Path $ResultsFile

Write-Host ""
Write-Host "Results saved to $ResultsFile" -ForegroundColor Yellow
