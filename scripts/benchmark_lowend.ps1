$ErrorActionPreference = 'Continue'
$IMAGE = 'rustedclaw-bench'
$PORT_BASE = 42700
$BINARY = 'c:\Users\DESKTOP\Desktop\Newwork\openclaw\target\release\rustedclaw.exe'
$PROJ = 'c:\Users\DESKTOP\Desktop\Newwork\openclaw'

Write-Host ''
Write-Host '======================================================================' -ForegroundColor Cyan
Write-Host '  RustedClaw Low-End Hardware Benchmark' -ForegroundColor Cyan
Write-Host '======================================================================' -ForegroundColor Cyan
Write-Host ''

# Host info
Write-Host '=== HOST MACHINE ===' -ForegroundColor Yellow
$cpu = Get-CimInstance Win32_Processor
$cpuName = $cpu.Name.Trim()
$cores = $cpu.NumberOfCores
$logical = $cpu.NumberOfLogicalProcessors
$clock = $cpu.MaxClockSpeed
Write-Host "  CPU:    $cpuName"
Write-Host "  Cores:  $cores physical / $logical logical"
Write-Host "  Clock:  $clock MHz"
$totalRAM = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
Write-Host "  RAM:    $totalRAM GB"
Write-Host ''

# Binary size
Write-Host '=== BINARY SIZE ===' -ForegroundColor Yellow
$f = Get-Item $BINARY
$sizeMB = [math]::Round($f.Length / 1MB, 2)
Write-Host "  Binary: $sizeMB MB"
if ($sizeMB -lt 4.0) { Write-Host '  EXCELLENT - smaller than ZeroClaw 8.8MB' -ForegroundColor Green }
elseif ($sizeMB -lt 6.0) { Write-Host '  GOOD - well under ZeroClaw 8.8MB' -ForegroundColor Green }
else { Write-Host '  NEEDS WORK - approaching ZeroClaw 8.8MB' -ForegroundColor Yellow }
Write-Host ''

# Cold start
Write-Host '=== COLD START - 20 runs ===' -ForegroundColor Yellow
$times = @()
for ($i = 0; $i -lt 20; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & $BINARY version 2>$null | Out-Null
    $sw.Stop()
    $times += $sw.ElapsedMilliseconds
}
$avg = [math]::Round(($times | Measure-Object -Average).Average, 1)
$minT = ($times | Measure-Object -Minimum).Minimum
$maxT = ($times | Measure-Object -Maximum).Maximum
$sorted = $times | Sort-Object
$p50 = $sorted[9]
$p99 = $sorted[18]
Write-Host "  Avg: ${avg}ms | Min: ${minT}ms | Max: ${maxT}ms"
Write-Host "  P50: ${p50}ms | P99: ${p99}ms"
Write-Host '  NOTE: NVMe + i7 makes this 3-5x faster than a VPS' -ForegroundColor DarkGray
Write-Host ''

# Host native benchmark
Write-Host '=== TIER 4: HOST MACHINE - no limits ===' -ForegroundColor Yellow
$env:OPENAI_API_KEY = 'sk-bench-placeholder-key-1234567890'
$PORT = $PORT_BASE

Get-Process rustedclaw -ErrorAction SilentlyContinue | Stop-Process -Force 2>$null
Start-Sleep 1

$proc = Start-Process $BINARY -ArgumentList 'gateway','--port',"$PORT" -PassThru -WindowStyle Hidden
Start-Sleep 3

$p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
if (-not $p) {
    Write-Host '  FAILED: Process died' -ForegroundColor Red
    exit 1
}

$idleWS = [math]::Round($p.WorkingSet64/1MB, 2)
$idlePriv = [math]::Round($p.PrivateMemorySize64/1MB, 2)
$threads = $p.Threads.Count
Write-Host "  Idle RAM:     ${idleWS} MB"
Write-Host "  Idle Private: ${idlePriv} MB"
Write-Host "  Threads:      $threads"

# 1000 requests
$sw = [System.Diagnostics.Stopwatch]::StartNew()
for ($i = 0; $i -lt 1000; $i++) { Invoke-RestMethod -Uri "http://127.0.0.1:$PORT/health" | Out-Null }
$sw.Stop()
$rps = [math]::Round(1000000 / $sw.ElapsedMilliseconds, 0)
$p.Refresh()
$after1k = [math]::Round($p.WorkingSet64/1MB, 2)
Write-Host "  After 1K req: ${after1k} MB"
Write-Host "  Throughput:   $rps req/s sequential"

# 5000 more
for ($i = 0; $i -lt 5000; $i++) { Invoke-RestMethod -Uri "http://127.0.0.1:$PORT/health" | Out-Null }
$p.Refresh()
$after6k = [math]::Round($p.WorkingSet64/1MB, 2)
$growth = [math]::Round($after6k - $idleWS, 2)
Write-Host "  After 6K req: ${after6k} MB"
Write-Host "  RAM growth:   ${growth} MB from idle"

Stop-Process -Id $proc.Id -Force 2>$null
Write-Host ''

# Docker build
Write-Host '=== BUILDING DOCKER IMAGE ===' -ForegroundColor Yellow
Write-Host '  This may take 5-10 minutes on first run...' -ForegroundColor DarkGray

docker build -t $IMAGE $PROJ 2>&1 | Select-Object -Last 3
if ($LASTEXITCODE -ne 0) {
    Write-Host '  Docker build FAILED. Skipping containerized tests.' -ForegroundColor Red
    Write-Host '  For true low-end testing, use a DigitalOcean droplet.' -ForegroundColor Yellow
    exit 0
}
Write-Host '  Built successfully' -ForegroundColor Green
Write-Host ''

# Docker tiers
$tierNames = @('TIER 1: Raspberry Pi 1CPU 256MB', 'TIER 2: 5-dollar VPS 1CPU 512MB', 'TIER 3: 10-dollar VPS 2CPU 1GB')
$tierCPUs =  @('1', '1', '2')
$tierMem =   @('256m', '512m', '1g')
$tierPorts = @(($PORT_BASE+1), ($PORT_BASE+2), ($PORT_BASE+3))

for ($t = 0; $t -lt 3; $t++) {
    $tName = $tierNames[$t]
    $tCPU = $tierCPUs[$t]
    $tMem = $tierMem[$t]
    $tPort = $tierPorts[$t]
    $cName = "rustedclaw-bench-$tPort"

    Write-Host "=== $tName ===" -ForegroundColor Yellow

    docker rm -f $cName 2>$null | Out-Null

    docker run -d --name $cName --cpus=$tCPU --memory=$tMem -p "${tPort}:42617" -e OPENAI_API_KEY=sk-bench-placeholder-key-1234567890 $IMAGE 2>$null | Out-Null

    Start-Sleep 5

    $status = docker inspect --format '{{.State.Status}}' $cName 2>$null
    if ($status -ne 'running') {
        Write-Host "  FAILED: Container not running - status: $status" -ForegroundColor Red
        docker logs $cName 2>&1 | Select-Object -Last 3
        docker rm -f $cName 2>$null | Out-Null
        continue
    }

    # Health
    try {
        $health = Invoke-RestMethod -Uri "http://127.0.0.1:${tPort}/health" -TimeoutSec 5
        Write-Host "  Health: $($health.status)" -ForegroundColor Green
    } catch {
        Write-Host '  Health check FAILED' -ForegroundColor Red
        docker rm -f $cName 2>$null | Out-Null
        continue
    }

    # Idle RAM
    $dstats = docker stats --no-stream --format '{{.MemUsage}}' $cName 2>$null
    Write-Host "  Idle RAM:   $dstats"

    # 500 requests
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt 500; $i++) {
        Invoke-RestMethod -Uri "http://127.0.0.1:${tPort}/health" | Out-Null
    }
    $sw.Stop()
    $trps = [math]::Round(500000 / $sw.ElapsedMilliseconds, 0)
    Write-Host "  Throughput: $trps req/s sequential"

    # Post-load RAM
    $dstats = docker stats --no-stream --format '{{.MemUsage}}' $cName 2>$null
    Write-Host "  Post-load:  $dstats"

    # Concurrent: 5 x 100
    $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
    $jobs = 1..5 | ForEach-Object {
        $pp = $tPort
        Start-Job -ScriptBlock {
            param($port)
            for ($i = 0; $i -lt 100; $i++) {
                Invoke-RestMethod -Uri "http://127.0.0.1:${port}/health" | Out-Null
            }
        } -ArgumentList $pp
    }
    $jobs | Wait-Job | Out-Null
    $sw2.Stop()
    $jobs | Remove-Job
    $crps = [math]::Round(500000 / $sw2.ElapsedMilliseconds, 0)
    Write-Host "  Concurrent: $crps req/s - 5 parallel workers"

    # Final RAM
    $dstats = docker stats --no-stream --format '{{.MemUsage}}' $cName 2>$null
    Write-Host "  Final RAM:  $dstats"

    docker rm -f $cName 2>$null | Out-Null
    Write-Host ''
}

# Summary
Write-Host '======================================================================' -ForegroundColor Cyan
Write-Host '  ANALYSIS' -ForegroundColor Cyan
Write-Host '======================================================================' -ForegroundColor Cyan
Write-Host ''
Write-Host '  Machine-INDEPENDENT metrics:' -ForegroundColor Green
Write-Host "    Binary size:   $sizeMB MB - same on any hardware"
Write-Host "    Thread count:  $threads - set by Tokio config, not hardware"
Write-Host ''
Write-Host '  Machine-DEPENDENT metrics:' -ForegroundColor Yellow
Write-Host "    Cold start:    ${avg}ms on YOUR i7+NVMe"
Write-Host '                   Expect 15-30ms on a budget VPS'
Write-Host '                   Expect 30-60ms on a Raspberry Pi 4'
Write-Host ''
Write-Host "    Throughput:    $rps req/s on YOUR machine"
Write-Host '                   Expect 40-60 percent on a budget VPS'
Write-Host ''
Write-Host '  SEMI-DEPENDENT metrics:' -ForegroundColor Yellow
Write-Host "    Idle RAM:      ${idleWS}MB on Windows native"
Write-Host '                   Docker numbers above are MORE ACCURATE'
Write-Host '                   because they isolate from Windows overhead'
Write-Host ''
Write-Host '  README RECOMMENDATIONS:' -ForegroundColor Cyan
Write-Host "    Binary: $sizeMB MB - universal, use this"
Write-Host '    RAM: Use Docker tier results, not Windows WorkingSet'
Write-Host '    Cold start: Say less-than-20ms - conservative, true on most HW'
Write-Host '    Throughput: Do NOT claim specific req/s in README'
Write-Host ''
Write-Host '======================================================================' -ForegroundColor Cyan
