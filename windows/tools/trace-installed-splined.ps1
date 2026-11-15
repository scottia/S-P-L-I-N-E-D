[CmdletBinding(DefaultParameterSetName = 'Launch')]
param(
    [Parameter(ParameterSetName = 'Launch')]
    [string]$ExecutablePath = 'E:\apps\SPLINED\splined.exe',

    [Parameter(Mandatory, ParameterSetName = 'Attach')]
    [int]$ProcessId,

    [string]$DatabasePath = 'E:\apps\SPLINED\_db\splined.db',
    [string]$OutputDirectory = 'E:\apps\SPLINED\diagnostics\runtime-monitor',
    [ValidateRange(5, 3600)]
    [int]$DurationSeconds = 120,
    [ValidateRange(100, 10000)]
    [int]$SampleMilliseconds = 500
)

$ErrorActionPreference = 'Stop'
$resolvedOutput = [System.IO.Path]::GetFullPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($resolvedOutput) | Out-Null
$runId = [DateTimeOffset]::Now.ToString('yyyyMMdd-HHmmss')
$runDirectory = Join-Path $resolvedOutput $runId
[System.IO.Directory]::CreateDirectory($runDirectory) | Out-Null
$startupTrace = Join-Path $runDirectory 'startup.tsv'
$sqlTrace = Join-Path $runDirectory 'sqlite.tsv'

if ($PSCmdlet.ParameterSetName -eq 'Launch') {
    $resolvedExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
    $priorStartupTrace = $env:SPLINED_STARTUP_TRACE_PATH
    $priorSqlTrace = $env:SPLINED_SQL_TRACE_PATH
    try {
        $env:SPLINED_STARTUP_TRACE_PATH = $startupTrace
        $env:SPLINED_SQL_TRACE_PATH = $sqlTrace
        $process = Start-Process -FilePath $resolvedExecutable -PassThru
    }
    finally {
        $env:SPLINED_STARTUP_TRACE_PATH = $priorStartupTrace
        $env:SPLINED_SQL_TRACE_PATH = $priorSqlTrace
    }
}
else {
    $process = Get-Process -Id $ProcessId
    $resolvedExecutable = $process.Path
}

$executableHash = (Get-FileHash -LiteralPath $resolvedExecutable -Algorithm SHA256).Hash
$metadata = [ordered]@{
    run_id = $runId
    mode = $PSCmdlet.ParameterSetName
    process_id = $process.Id
    executable = $resolvedExecutable
    executable_sha256 = $executableHash
    database = [System.IO.Path]::GetFullPath($DatabasePath)
    started_utc = [DateTimeOffset]::UtcNow.ToString('O')
    duration_seconds = $DurationSeconds
    sample_milliseconds = $SampleMilliseconds
}
$metadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $runDirectory 'run.json') -Encoding utf8

$processSamples = [System.Collections.Generic.List[object]]::new()
$threadSamples = [System.Collections.Generic.List[object]]::new()
$priorThreadCpu = @{}
$deadline = [DateTimeOffset]::UtcNow.AddSeconds($DurationSeconds)
$databaseFiles = @($DatabasePath, "$DatabasePath-wal", "$DatabasePath-shm")

while ([DateTimeOffset]::UtcNow -lt $deadline) {
    $current = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
    if (-not $current) {
        break
    }
    $current.Refresh()
    $now = [DateTimeOffset]::UtcNow
    foreach ($databaseFile in $databaseFiles) {
        $item = Get-Item -LiteralPath $databaseFile -ErrorAction SilentlyContinue
        $processSamples.Add([pscustomobject]@{
            timestamp_utc = $now.ToString('O')
            process_id = $current.Id
            responding = $current.Responding
            cpu_seconds = $current.CPU
            working_set_bytes = $current.WorkingSet64
            private_bytes = $current.PrivateMemorySize64
            handle_count = $current.HandleCount
            thread_count = $current.Threads.Count
            database_file = $databaseFile
            database_exists = [bool]$item
            database_bytes = if ($item) { $item.Length } else { 0 }
            database_last_write_utc = if ($item) { $item.LastWriteTimeUtc.ToString('O') } else { '' }
        })
    }
    foreach ($thread in $current.Threads) {
        $totalMs = $thread.TotalProcessorTime.TotalMilliseconds
        $priorMs = if ($priorThreadCpu.ContainsKey($thread.Id)) { $priorThreadCpu[$thread.Id] } else { $totalMs }
        $priorThreadCpu[$thread.Id] = $totalMs
        $threadSamples.Add([pscustomobject]@{
            timestamp_utc = $now.ToString('O')
            process_id = $current.Id
            thread_id = $thread.Id
            state = $thread.ThreadState
            wait_reason = if ($thread.ThreadState -eq 'Wait') { $thread.WaitReason } else { '' }
            total_cpu_ms = $totalMs
            delta_cpu_ms = $totalMs - $priorMs
            user_cpu_ms = $thread.UserProcessorTime.TotalMilliseconds
            kernel_cpu_ms = $thread.PrivilegedProcessorTime.TotalMilliseconds
        })
    }
    Start-Sleep -Milliseconds $SampleMilliseconds
}

$processSamples | Export-Csv -LiteralPath (Join-Path $runDirectory 'process-and-sqlite.csv') -NoTypeInformation -Encoding utf8
$threadSamples | Export-Csv -LiteralPath (Join-Path $runDirectory 'threads.csv') -NoTypeInformation -Encoding utf8
$metadata['ended_utc'] = [DateTimeOffset]::UtcNow.ToString('O')
$metadata['process_still_running'] = [bool](Get-Process -Id $process.Id -ErrorAction SilentlyContinue)
$metadata | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $runDirectory 'run.json') -Encoding utf8

[pscustomobject]@{
    ProcessId = $process.Id
    ExecutableSha256 = $executableHash
    RunDirectory = $runDirectory
    StartupTrace = $startupTrace
    SqlTrace = $sqlTrace
    ProcessSamples = Join-Path $runDirectory 'process-and-sqlite.csv'
    ThreadSamples = Join-Path $runDirectory 'threads.csv'
}
