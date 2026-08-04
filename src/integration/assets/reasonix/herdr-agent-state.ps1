# installed by herdr
# managed by herdr; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# HERDR_INTEGRATION_ID=reasonix
# HERDR_INTEGRATION_VERSION=1

param([string]$Action = "")

if (@("session", "working", "blocked", "idle") -notcontains $Action) { exit 0 }
if ($env:HERDR_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:HERDR_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    $payload = if ([string]::IsNullOrWhiteSpace($inputText)) { $null } else { $inputText | ConvertFrom-Json }
} catch {
    $payload = $null
}

$seq = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sessionId = if ($null -ne $payload -and -not [string]::IsNullOrWhiteSpace($payload.sessionId)) { $payload.sessionId } else { $null }
if ([string]::IsNullOrWhiteSpace($sessionId) -and $null -ne $payload) { $sessionId = $payload.session_id }

try {
    if ($Action -eq "session") {
        if ([string]::IsNullOrWhiteSpace($sessionId)) { exit 0 }
        & herdr pane report-agent-session $env:HERDR_PANE_ID --source herdr:reasonix --agent reasonix --agent-session-id $sessionId --session-start-source startup --seq $seq 2>$null | Out-Null
    } else {
        if ([string]::IsNullOrWhiteSpace($sessionId)) {
            & herdr pane report-agent $env:HERDR_PANE_ID --source herdr:reasonix --agent reasonix --state $Action --seq $seq 2>$null | Out-Null
        } else {
            & herdr pane report-agent $env:HERDR_PANE_ID --source herdr:reasonix --agent reasonix --state $Action --agent-session-id $sessionId --seq $seq 2>$null | Out-Null
        }
    }
} catch {
}
