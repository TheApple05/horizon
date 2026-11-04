param(
    [Parameter(Mandatory = $true)][string]$Config,
    [string]$TaskName = 'Horizon Fork Updater',
    [switch]$Start
)
$ErrorActionPreference = 'Stop'
$configuration = (Resolve-Path -LiteralPath $Config).Path
$settings = Get-Content -LiteralPath $configuration -Raw | ConvertFrom-Json
$controlRoot = Join-Path (Split-Path -Parent $configuration) 'control'
New-Item -ItemType Directory -Path $controlRoot -Force | Out-Null
$scriptNames = @('fork_updater.py', 'fork_updater_service.py', 'fork_build_candidate.py',
    'fork_runtime_smoke.py', 'fork_photon_gate.py', 'fork_terra_gate.py', 'lab.py')
foreach ($scriptName in $scriptNames) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $scriptName) -Destination (Join-Path $controlRoot $scriptName) -Force
}
Copy-Item -LiteralPath (Join-Path (Split-Path -Parent $PSScriptRoot) 'research-lock.json') -Destination (Join-Path (Split-Path -Parent $configuration) 'research-lock.json') -Force
$python = (Get-Command python.exe -ErrorAction Stop).Source
$pythonWindowless = Join-Path (Split-Path -Parent $python) 'pythonw.exe'
if (-not (Test-Path -LiteralPath $pythonWindowless -PathType Leaf)) {
    throw 'The Python installation must provide pythonw.exe for the hidden worker.'
}
$service = Join-Path $controlRoot 'fork_updater_service.py'
$action = New-ScheduledTaskAction -Execute $pythonWindowless -Argument ('"{0}" --config "{1}" --interval 120' -f $service, $configuration) -WorkingDirectory (Split-Path -Parent $configuration)
$triggers = @(
    (New-ScheduledTaskTrigger -AtLogOn -User ([Security.Principal.WindowsIdentity]::GetCurrent().Name)),
    (New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(5) -RepetitionInterval (New-TimeSpan -Minutes 5))
)
$principal = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited
$taskSettings = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero) -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $triggers -Principal $principal -Settings $taskSettings -Description 'Durable per-commit Horizon upstream compatibility worker. Reports through the Codex heartbeat.' -Force | Out-Null
if ($Start) { Start-ScheduledTask -TaskName $TaskName }
Get-ScheduledTask -TaskName $TaskName | Select-Object TaskName, State
