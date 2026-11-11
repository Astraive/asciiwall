#requires -Version 7.0
[CmdletBinding()]
param(
    [string] $BacklogPath = (Join-Path $PSScriptRoot '../../.issues'),
    [switch] $Apply,
    [switch] $DryRun,
    [switch] $VerifyOnly,
    [string] $GhCommand = 'gh',
    [string] $PythonCommand = 'python',
    [string] $Owner = 'Astraive',
    [int] $ProjectNumber = 4,
    [string] $SourceRef = '',
    [string[]] $Milestone = @(),
    [string] $ExpectedLogin = '',
    [ValidateRange(1, 10)][int] $ReadAttempts = 4,
    [ValidateRange(1, 3600)][int] $CommandTimeoutSeconds = 180,
    [string[]] $RetryOperation = @()
)

$ErrorActionPreference = 'Stop'
$script:RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$script:BacklogRoot = [IO.Path]::GetFullPath($BacklogPath)
$script:Utf8 = [Text.UTF8Encoding]::new($false)
$script:LedgerPath = Join-Path $script:BacklogRoot 'publication-ledger.json'
$script:Ledger = $null
$script:Lock = $null
$script:Tickets = @{}
$script:Identities = @{}
$script:TicketsById = $null
$script:TicketsByDirectory = $null
$script:Milestones = @{}
$script:Fields = @{}
$script:ProjectItems = @{}
$script:Failures = [Collections.Generic.List[string]]::new()
$script:Inventory = @{}
$script:Schema = @{}
$script:AttemptedOperations = @{}
$script:DerivedLabels = @()
$script:DerivedLabelSet = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:DerivedTicketLabels = [hashtable]::new([StringComparer]::Ordinal)
$script:PublicationKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:UsedMilestones = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:ManagedMilestones = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:CurrentOperations = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:RequestedMilestones = [string[]] @($Milestone)

function Json($Value) { ConvertTo-Json -InputObject $Value -Depth 100 -Compress }
function Hash([string] $Text) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($algorithm.ComputeHash($script:Utf8.GetBytes($Text))).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose() }
}
function Read-Json([string] $Path) { Get-Content -LiteralPath $Path -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable }
function Escape-Segment([string] $Value) { [Uri]::EscapeDataString($Value) }
function Key($Ticket) { "$($Ticket.repository)/$($Ticket.id)" }
function Marker($Ticket) { "<!-- asciiwall-backlog:v1 repository=$($Ticket.repository) ticket=$($Ticket.id) -->" }
function Assert([bool] $Condition, [string] $Message) { if (-not $Condition) { throw $Message } }
function Is-JsonArray($Value) {
    $null -ne $Value -and $Value -is [Collections.IList] -and $Value -isnot [string] -and $Value -isnot [Collections.IDictionary]
}
function Same-Set($Left, $Right) {
    $a = @($Left | Where-Object { $null -ne $_ -and [string] $_ -ne '' } | ForEach-Object { [string] $_ } | Sort-Object -Unique)
    $b = @($Right | Where-Object { $null -ne $_ -and [string] $_ -ne '' } | ForEach-Object { [string] $_ } | Sort-Object -Unique)
    (Json $a) -ceq (Json $b)
}
function Assert-ExternalRoot {
    Assert (Test-Path -LiteralPath $script:BacklogRoot -PathType Container) "Backlog directory does not exist: $script:BacklogRoot"
    $library = [IO.Path]::GetFullPath((Join-Path $script:RepoRoot '../asciiwall-library'))
    foreach ($root in @($script:RepoRoot, $library)) {
        $prefix = $root.TrimEnd('/', '\') + [IO.Path]::DirectorySeparatorChar
        Assert (-not ($script:BacklogRoot.Equals($root, [StringComparison]::OrdinalIgnoreCase) -or $script:BacklogRoot.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase))) "Backlog must be external to both repositories: $script:BacklogRoot"
    }
    $directory = [IO.DirectoryInfo]::new($script:BacklogRoot)
    while ($null -ne $directory) {
        Assert (($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) "Backlog ancestors must not be reparse points/symlinks: $($directory.FullName)"
        $directory = $directory.Parent
    }
    foreach ($entry in Get-ChildItem -LiteralPath $script:BacklogRoot -Recurse -Force) {
        Assert (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) "Backlog must not contain reparse points/symlinks: $($entry.FullName)"
    }
}
function Run-Process([string] $Executable, [string[]] $Arguments, [string] $InputText = '') {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardInputEncoding = $script:Utf8
    $start.StandardOutputEncoding = $script:Utf8
    $start.StandardErrorEncoding = $script:Utf8
    $start.WorkingDirectory = $script:RepoRoot
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        [void] $process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.StandardInput.Write($InputText)
        $process.StandardInput.Close()
        if (-not $process.WaitForExit($CommandTimeoutSeconds * 1000)) {
            $process.Kill($true)
            throw "Executable timed out after ${CommandTimeoutSeconds}s: $Executable; a mutation may have reached GitHub."
        }
        @{ ExitCode = $process.ExitCode; Out = $stdout.GetAwaiter().GetResult(); Error = $stderr.GetAwaiter().GetResult() }
    } finally { $process.Dispose() }
}
function Invoke-Api([string] $Method, [string] $Endpoint, $Payload = $null, [switch] $ReadOnly, [switch] $AllowNotFound) {
    $arguments = @('api', '--hostname', 'github.com', '--include', '--method', $Method, '-H', 'Accept: application/vnd.github+json', '-H', 'X-GitHub-Api-Version: 2022-11-28', $Endpoint)
    $inputText = ''
    if ($null -ne $Payload) { $arguments += @('--input', '-'); $inputText = Json $Payload }
    $attempts = if ($ReadOnly) { $ReadAttempts } else { 1 }
    for ($attempt = 1; $attempt -le $attempts; $attempt++) {
        $result = Run-Process $GhCommand $arguments $inputText
        $text = $result.Out
        $headers = @{}
        $status = 0
        # gh --include supplies HTTP headers; require them, including in fake-gh boundary tests.
        while ($text -match '\AHTTP/\S+\s+(\d{3})[^\r\n]*\r?\n') {
            $status = [int] $Matches[1]
            $boundary = [regex]::Match($text, '\r?\n\r?\n')
            Assert $boundary.Success "Malformed gh HTTP response for $Method $Endpoint"
            foreach ($line in ($text.Substring(0, $boundary.Index) -split '\r?\n' | Select-Object -Skip 1)) {
                if ($line -match '^([^:]+):\s*(.*)$') { $headers[$Matches[1].ToLowerInvariant()] = $Matches[2] }
            }
            $text = $text.Substring($boundary.Index + $boundary.Length)
        }
        if ($AllowNotFound -and $status -eq 404) { return $null }
        $body = $null
        if (-not [string]::IsNullOrWhiteSpace($text)) {
            try { $body = $text | ConvertFrom-Json -AsHashtable -NoEnumerate }
            catch { if ($status -ge 200 -and $status -lt 300) { throw "Non-JSON gh response for $Method $Endpoint (HTTP $status): $text" } }
        }
        $graphErrors = $null -ne $body -and $body -is [Collections.IDictionary] -and $body.Contains('errors')
        if ($result.ExitCode -eq 0 -and $status -ge 200 -and $status -lt 300 -and -not $graphErrors) { return $body }
        $rateLimited = $status -eq 429 -or ($graphErrors -and 'RATE_LIMITED' -in @($body.errors.type)) -or ($status -eq 403 -and ($headers['x-ratelimit-remaining'] -eq '0' -or $headers.ContainsKey('retry-after') -or $text -match '(?i)secondary rate limit'))
        $transient = $rateLimited -or $status -in @(0, 502, 503, 504)
        if ($ReadOnly -and $transient -and $attempt -lt $attempts) {
            $delay = [Math]::Pow(2, $attempt)
            if ($headers['retry-after'] -match '^\d+$') { $delay = [double] $headers['retry-after'] }
            elseif ($headers.ContainsKey('retry-after')) {
                $retryAt = [DateTimeOffset]::MinValue
                Assert ([DateTimeOffset]::TryParse($headers['retry-after'], [ref] $retryAt)) "Invalid Retry-After for $Method $Endpoint; stopping rather than ignoring the rate limit."
                $delay = [Math]::Max(0, ($retryAt - [DateTimeOffset]::UtcNow).TotalSeconds)
            }
            elseif ($rateLimited -and $headers['x-ratelimit-reset'] -match '^\d+$') {
                $delay = [Math]::Max($delay, [double] $headers['x-ratelimit-reset'] - [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() + 1)
            }
            Assert ($delay -le 60) "Read rate limit requires ${delay}s wait; stopping without mutation retry: $Method $Endpoint"
            Start-Sleep -Seconds ([int][Math]::Ceiling($delay))
            continue
        }
        throw "GitHub $Method $Endpoint failed (exit $($result.ExitCode), HTTP $status): $text $($result.Error)"
    }
}
function Read-Api([string] $Endpoint, [switch] $AllowNotFound) { Invoke-Api 'GET' $Endpoint -ReadOnly -AllowNotFound:$AllowNotFound }
function Read-Pages([string] $Endpoint) {
    $rows = [Collections.Generic.List[object]]::new()
    $separator = if ($Endpoint.Contains('?')) { '&' } else { '?' }
    for ($page = 1; ; $page++) {
        $part = @(Read-Api "${Endpoint}${separator}per_page=100&page=$page")
        foreach ($row in $part) { $rows.Add($row) }
        if ($part.Count -lt 100) { break }
    }
    return $rows.ToArray()
}
function Graph-Read([string] $Query, [hashtable] $Variables = @{}) {
    $response = Invoke-Api 'POST' 'graphql' @{ query = $Query; variables = $Variables } -ReadOnly
    $response.data
}
function Save-Ledger {
    Assert ([bool] $Apply -and $null -ne $script:Lock) 'Ledger writes require Apply and the exclusive lock.'
    $script:Ledger.updated_at = [DateTimeOffset]::UtcNow.ToString('o')
    $temp = "$script:LedgerPath.$([Guid]::NewGuid().ToString('N')).tmp"
    $stream = $null
    try {
        $stream = [IO.FileStream]::new($temp, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $bytes = $script:Utf8.GetBytes((Json $script:Ledger) + "`n")
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
        $stream.Dispose(); $stream = $null
        [IO.File]::Move($temp, $script:LedgerPath, $true)
    } finally {
        if ($null -ne $stream) { $stream.Dispose() }
        if ([IO.File]::Exists($temp)) { [IO.File]::Delete($temp) }
    }
}
function Set-Reconciled([string] $Operation) {
    [void] $script:CurrentOperations.Add($Operation)
    if ($Apply -and $script:Ledger.operations.ContainsKey($Operation)) {
        $script:Ledger.operations[$Operation].state = 'reconciled'
        $script:Ledger.operations[$Operation].resolved_at = [DateTimeOffset]::UtcNow.ToString('o')
        Save-Ledger
    }
}
function Mutate([string] $Operation, [string] $Method, [string] $Endpoint, $Payload) {
    [void] $script:CurrentOperations.Add($Operation)
    Assert ([bool] $Apply) "Read-only mode requires pending operation: $Operation $Method $Endpoint $(Json $Payload)"
    Assert (-not $script:AttemptedOperations.ContainsKey($Operation)) "A mutation for '$Operation' was already attempted in this process; refusing a second request."
    $old = $script:Ledger.operations[$Operation]
    if ($null -ne $old -and $old.state -in @('intent', 'unknown', 'succeeded') -and $Operation -cnotin $RetryOperation) {
        throw "Unreconciled durable operation '$Operation'. Refetch did not prove its desired result. Inspect remote state; only after proving it did not apply, authorize one explicit attempt with -RetryOperation '$Operation'. Pending: $Method $Endpoint $(Json $Payload)"
    }
    $script:AttemptedOperations[$Operation] = $true
    $script:Ledger.operations[$Operation] = @{
        state = 'intent'; method = $Method; endpoint = $Endpoint; payload = $Payload
        payload_sha256 = Hash (Json $Payload); started_at = [DateTimeOffset]::UtcNow.ToString('o')
        previous_attempt = $(if ($old) { @{ state = $old.state; started_at = $old.started_at; payload_sha256 = $old.payload_sha256; error = $old.error } } else { $null })
    }
    Save-Ledger
    Start-Sleep -Milliseconds 1050
    try {
        $response = Invoke-Api $Method $Endpoint $Payload
        $script:Ledger.operations[$Operation].state = 'succeeded'
        $script:Ledger.operations[$Operation].completed_at = [DateTimeOffset]::UtcNow.ToString('o')
        if ($response -is [Collections.IDictionary] -and $response.Contains('data')) { $receipt = $response.data }
        else {
            $receipt = @{}
            foreach ($property in @('id', 'node_id', 'number', 'html_url', 'name', 'title', 'state')) {
                if ($response -is [Collections.IDictionary] -and $response.Contains($property)) { $receipt[$property] = $response[$property] }
            }
        }
        $script:Ledger.operations[$Operation].response = $receipt
        Save-Ledger
        return $response
    } catch {
        $script:Ledger.operations[$Operation].state = 'unknown'
        $script:Ledger.operations[$Operation].error = $_.Exception.Message
        Save-Ledger
        throw "Pending operation '$Operation': $Method $Endpoint $(Json $Payload)`n$($_.Exception.Message)"
    }
}
function Graph-Mutate([string] $Operation, [string] $Query, [hashtable] $Variables) {
    $response = Mutate $Operation 'POST' 'graphql' @{ query = $Query; variables = $Variables }
    $response.data
}
function Resolve-Ticket([string] $Reference, $From) {
    if ([string]::IsNullOrWhiteSpace($Reference)) { return $null }
    $referenceKey = $Reference -replace ('^' + [regex]::Escape($Owner) + '/'), ''
    if ($script:Tickets.ContainsKey($referenceKey)) { return $script:Tickets[$referenceKey] }
    $local = "$($From.repository)/$Reference"
    if ($script:Tickets.ContainsKey($local)) { return $script:Tickets[$local] }
    if ($null -ne $script:TicketsById -and $script:TicketsById.ContainsKey($Reference)) {
        return $script:TicketsById[$Reference]
    }
    Assert $false "Unknown or ambiguous ticket reference '$Reference' from $(Key $From)"
}
function Native-Type($Ticket) {
    switch ($Ticket.type.ToLowerInvariant()) { 'bug' { 'Bug' }; 'feature' { 'Feature' }; 'enhancement' { 'Feature' }; default { 'Task' } }
}
function Is-ManagedLabel([string] $Name) {
    if ([string]::IsNullOrEmpty($Name)) { return $false }
    $Name -match '^(?:kind|priority|size|type|area|platform):' -or $script:DerivedLabelSet.Contains($Name)
}
function Desired-Labels($Ticket) {
    $labels = @("kind:$($Ticket.kind)", "priority:$($Ticket.priority)", "size:$($Ticket.size)", "type:$($Ticket.type)", "area:$($Ticket.area)") + @($Ticket.platforms | ForEach-Object { "platform:$_" })
    if ($script:DerivedTicketLabels.ContainsKey($Ticket.id)) {
        $labels += @($script:DerivedTicketLabels[$Ticket.id])
    }
    $labels
}
function Milestone-Key($Ticket) { "$($Ticket.repository)/$($Ticket.milestone)" }
function Desired-Field($Field, $Ticket) {
    switch ($Field.name) {
        'Repo role' { if ($Ticket.repository -eq 'asciiwall') { return 'Main App' } else { return 'Wallpaper Library' } }
        'Blocked' {
            foreach ($dependency in $Ticket.depends_on) {
                $target = Resolve-Ticket $dependency $Ticket
                $identity = $script:Identities[(Key $target)]
                if ($null -eq $identity -or $identity.state -ne 'closed') { return 'Yes' }
            }
            return 'No'
        }
        'Target release' { return $script:MilestoneRegistry[(Milestone-Key $Ticket)].target_release }
    }
    $source = [string] $Field.source
    if (-not $Ticket.Contains($source)) { throw "Unsupported source '$source' for project field '$($Field.name)'" }
    $value = $Ticket[$source]
    if ($Field.kind -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
        $resolved = @(foreach ($item in @($value)) {
            $exact = @($Field.options | Where-Object { $_ -ieq [string] $item })
            Assert ($exact.Count -eq 1) "No unique '$item' option for $($Field.name) on $(Key $Ticket)"
            $exact[0]
        })
        if ($Field.kind -eq 'MULTI_SELECT') { return , [string[]] @($resolved) }
        return $resolved[0]
    }
    return $value
}
function Assert-BodyLimit([string] $Body, $Ticket, [string] $Kind) {
    Assert ($Body.Length -le 65536) "$Kind rendered body for $(Key $Ticket) exceeds GitHub's 65,536-character issue-body limit ($($Body.Length) characters)."
}
function Preflight-RenderedBodies {
    $saved = $script:Identities
    try {
        $script:Identities = @{}
        foreach ($ticket in $script:Order) {
            $script:Identities[(Key $ticket)] = @{ html_url = "https://github.com/$Owner/$($ticket.repository)/issues/9999999999"; state = 'open' }
        }
        foreach ($ticket in $script:Order) {
            $initial = Render-Body $ticket -Initial
            Assert-BodyLimit $initial $ticket 'Initial'
            $final = Render-Body $ticket
            Assert-BodyLimit $final $ticket 'Final preflight'
        }
    } finally { $script:Identities = $saved }
}
function Select-PublicationScope {
    $selected = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($key in $script:RequestedMilestones) {
        Assert ($script:MilestoneRegistry.ContainsKey($key)) "Unknown milestone '$key'; use a qualified key such as asciiwall/M0."
        [void] $script:ManagedMilestones.Add($key)
    }
    foreach ($ticket in $script:Order) {
        if ($script:RequestedMilestones.Count -eq 0 -or (Milestone-Key $ticket) -cin $script:RequestedMilestones) { [void] $selected.Add((Key $ticket)) }
    }
    Assert ($selected.Count -gt 0) 'Publication scope contains no tickets.'
    foreach ($key in @($selected)) {
        $ticket = $script:Tickets[$key]
        while ($ticket.parent) {
            $ticket = Resolve-Ticket $ticket.parent $ticket
            Assert ($ticket.kind -cin @('epic', 'roadmap')) "Required ancestor $(Key $ticket) is not a tracking container."
            [void] $selected.Add((Key $ticket))
        }
    }
    foreach ($key in $selected) {
        $ticket = $script:Tickets[$key]
        foreach ($dependency in $ticket.depends_on) {
            $required = Resolve-Ticket $dependency $ticket
            Assert ($selected.Contains((Key $required))) "Scope omits hard dependency $(Key $required) for $key; explicitly include its milestone. No later task is published automatically."
        }
        [void] $script:UsedMilestones.Add((Milestone-Key $ticket))
    }
    if ($script:RequestedMilestones.Count -eq 0) {
        foreach ($key in $script:MilestoneRegistry.Keys) { [void] $script:ManagedMilestones.Add($key) }
        foreach ($key in $script:MilestoneRegistry.Keys) { [void] $script:UsedMilestones.Add($key) }
    }
    $script:PublicationKeys = $selected
    $script:Order = @($script:Order | Where-Object { $selected.Contains((Key $_)) })
}

function Load-Source {
    $script:DerivedLabels = @()
    $script:DerivedLabelSet = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:DerivedTicketLabels = [hashtable]::new([StringComparer]::Ordinal)
    $script:LabelRegistry = @(Read-Json (Join-Path $script:BacklogRoot 'labels.json'))
    $script:ProjectRegistry = Read-Json (Join-Path $script:BacklogRoot 'project.json')
    $script:MilestoneRegistry = @{}
    foreach ($m in @(Read-Json (Join-Path $script:BacklogRoot 'milestones.json'))) {
        $key = "$($m.repository)/$($m.id)"
        Assert (-not $script:MilestoneRegistry.ContainsKey($key)) "Duplicate milestone $key"
        Assert ($m.repository -in @('asciiwall', 'asciiwall-library') -and -not [string]::IsNullOrWhiteSpace($m.title)) "Invalid milestone $key"
        Assert (@($script:MilestoneRegistry.Values | Where-Object { $_.repository -ceq $m.repository -and $_.title -ceq $m.title }).Count -eq 0) "Duplicate milestone title in $($m.repository): $($m.title)"
        $script:MilestoneRegistry[$key] = $m
    }
    foreach ($repository in @('asciiwall', 'asciiwall-library')) {
        $path = Join-Path $script:BacklogRoot $repository
        foreach ($file in Get-ChildItem -LiteralPath $path -Filter issue.json -Recurse -File) {
            $metadataText = Get-Content -LiteralPath $file.FullName -Raw -Encoding utf8
            $ticket = $metadataText | ConvertFrom-Json -AsHashtable
            Assert ($ticket.repository -ceq $repository -and $file.Directory.Name -ceq $ticket.id -and $file.Directory.Parent.FullName -eq $path) "Unexpected ticket identity/path: $($file.FullName)"
            $key = Key $ticket
            Assert (-not $script:Tickets.ContainsKey($key)) "Duplicate ticket $key"
            $ticket['_directory'] = $file.Directory.FullName
            $ticket['_body'] = Get-Content -LiteralPath (Join-Path $file.Directory.FullName 'CONTENT.md') -Raw -Encoding utf8
            $ticket['_source_hash'] = Hash ($metadataText + "`n" + $ticket._body)
            $script:Tickets[$key] = $ticket
        }
    }
    Assert ($script:Tickets.Count -gt 0) 'No source tickets.'
    $ticketsById = [hashtable]::new([StringComparer]::Ordinal)
    $directoryComparer = if ([OperatingSystem]::IsWindows()) { [StringComparer]::OrdinalIgnoreCase } else { [StringComparer]::Ordinal }
    $ticketsByDirectory = [Collections.Generic.Dictionary[string, Collections.Generic.List[object]]]::new($directoryComparer)
    foreach ($ticket in $script:Tickets.Values) {
        Assert ($ticket.id -is [string] -and -not [string]::IsNullOrWhiteSpace($ticket.id)) "Missing ticket id for $(Key $ticket)"
        Assert (-not $ticketsById.ContainsKey($ticket.id)) "Duplicate global ticket ID '$($ticket.id)' across repositories."
        $ticketsById[$ticket.id] = $ticket
        $directory = [IO.Path]::GetFullPath([string] $ticket._directory)
        if (-not $ticketsByDirectory.ContainsKey($directory)) {
            $ticketsByDirectory[$directory] = [Collections.Generic.List[object]]::new()
        }
        $ticketsByDirectory[$directory].Add($ticket)
    }
    $script:TicketsById = $ticketsById
    $script:TicketsByDirectory = $ticketsByDirectory
    $names = @{}
    $exactLabelNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($label in $script:LabelRegistry) {
        Assert (-not $names.ContainsKey($label.name) -and $label.color -match '^[0-9a-fA-F]{6}$' -and $label.description.Length -le 100 -and $label.name.Length -le 50) "Invalid/duplicate label: $(Json $label)"
        $names[$label.name] = $true
        [void] $exactLabelNames.Add([string] $label.name)
    }
    Assert ($script:ProjectRegistry.Contains('fields') -and (Is-JsonArray $script:ProjectRegistry['fields'])) 'Project registry fields must be an array.'
    $fieldNames = @{}
    foreach ($field in $script:ProjectRegistry.fields) {
        Assert (-not $fieldNames.ContainsKey($field.name)) "Duplicate project field $($field.name)"
        $fieldNames[$field.name] = $true
        Assert ($field.kind -in @('SINGLE_SELECT', 'MULTI_SELECT', 'TEXT', 'NUMBER') -or $field.kind -cmatch '^NATIVE_[A-Z][A-Z0-9_]*$') "Unsupported field kind $($field.kind)"
        if ($field.kind -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
            Assert (@($field.options).Count -gt 0 -and @($field.options).Count -le 50 -and @($field.options | Sort-Object -Unique).Count -eq @($field.options).Count) "Project select options must be unique and number 1..50: $($field.name)"
        }
        if ($field.kind -in @('TEXT', 'NUMBER')) { Assert (@($field.options).Count -eq 0) "Nonselect field cannot declare options: $($field.name)" }
    }
    foreach ($required in @('Status', 'Kind', 'Priority', 'Size', 'Work type', 'Area', 'Platform', 'Repository', 'Milestone', 'Assignees', 'Phase', 'Blocked', 'Target release', 'Effort')) {
        Assert ($fieldNames.ContainsKey($required)) "Missing required project field $required"
    }
    foreach ($native in @{ Repository = 'NATIVE_REPOSITORY'; Milestone = 'NATIVE_MILESTONE'; Assignees = 'NATIVE_ASSIGNEES' }.GetEnumerator()) {
        Assert (($script:ProjectRegistry.fields | Where-Object name -eq $native.Key).kind -ceq $native.Value) "Registry attempts to shadow native $($native.Key)."
    }
    Assert (@(($script:ProjectRegistry.fields | Where-Object name -eq 'Phase').options).Count -le 16) 'Phase must use at most 16 canonical values.'
    Assert ($script:ProjectRegistry.number -eq $ProjectNumber) 'Project registry number does not match requested project.'
    Assert ($script:ProjectRegistry.Contains('repositories') -and (Is-JsonArray $script:ProjectRegistry['repositories'])) 'Project registry repositories must be an array.'
    Assert (Same-Set $script:ProjectRegistry.repositories @("$Owner/asciiwall", "$Owner/asciiwall-library")) 'Project registry must cover exactly both repositories.'
    $derivedList = [Collections.Generic.List[object]]::new()
    if ($script:ProjectRegistry.Contains('derived_labels')) {
        $rawDerived = $script:ProjectRegistry['derived_labels']
        Assert (Is-JsonArray $rawDerived) 'Project registry derived_labels must be an array.'
        foreach ($entry in $rawDerived) {
            Assert ($null -ne $entry -and $entry -is [Collections.IDictionary]) 'Each project.derived_labels entry must be an object.'
            foreach ($prop in @('name', 'ticket_ids', 'source', 'meaning')) {
                Assert ($entry.Contains($prop)) "Project derived_labels entry is missing required property '$prop': $(Json $entry)"
            }
            Assert ($entry['name'] -is [string] -and -not [string]::IsNullOrWhiteSpace($entry['name'])) 'Derived label entry name must be a non-empty string.'
            $labelName = [string] $entry['name']
            Assert ($exactLabelNames.Contains($labelName)) "Derived label '$labelName' is not registered in labels.json."
            Assert ($labelName -notmatch '^(?:kind|priority|size|type|area|platform):') "Derived label '$labelName' conflicts with a built-in ticket label family."
            Assert ($script:DerivedLabelSet.Add($labelName)) "Duplicate derived label '$labelName' in project.derived_labels."
            Assert ($entry['source'] -is [string] -and -not [string]::IsNullOrWhiteSpace($entry['source'])) "Derived label '$labelName' source must be a non-empty string."
            Assert ($entry['meaning'] -is [string] -and -not [string]::IsNullOrWhiteSpace($entry['meaning'])) "Derived label '$labelName' meaning must be a non-empty string."
            if ($entry.Contains('repository')) {
                Assert ($entry['repository'] -is [string] -and $entry['repository'] -cin @('asciiwall', 'asciiwall-library', "$Owner/asciiwall", "$Owner/asciiwall-library")) "Derived label '$labelName' references foreign repository '$($entry['repository'])'."
            }
            if ($entry.Contains('repositories')) {
                Assert (Is-JsonArray $entry['repositories']) "Derived label '$labelName' repositories must be an array."
                foreach ($repo in $entry['repositories']) {
                    Assert ($repo -is [string] -and $repo -cin @('asciiwall', 'asciiwall-library', "$Owner/asciiwall", "$Owner/asciiwall-library")) "Derived label '$labelName' references foreign repository '$repo'."
                }
            }
            $rawIds = $entry['ticket_ids']
            Assert (Is-JsonArray $rawIds) "Derived label '$labelName' ticket_ids must be an array."
            $seenIds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            $normalizedIds = [Collections.Generic.List[string]]::new()
            foreach ($rawId in $rawIds) {
                Assert ($rawId -is [string] -and -not [string]::IsNullOrWhiteSpace($rawId)) "Derived label '$labelName' ticket_ids entries must be non-empty strings."
                $ticketId = [string] $rawId
                if ($ticketId.Contains('/')) {
                    $repoPrefix = $ticketId.Substring(0, $ticketId.LastIndexOf('/'))
                    Assert $false "Derived label '$labelName' ticket_id '$ticketId' uses a repository prefix ('$repoPrefix'); only bare globally unique ticket IDs from asciiwall or asciiwall-library are allowed (no foreign repositories)."
                }
                Assert ($ticketsById.ContainsKey($ticketId)) "Derived label '$labelName' references unknown ticket ID '$ticketId'."
                $targetTicket = $ticketsById[$ticketId]
                Assert ($targetTicket.repository -cin @('asciiwall', 'asciiwall-library')) "Derived label '$labelName' references foreign repository '$($targetTicket.repository)' on '$ticketId'."
                Assert ($seenIds.Add($ticketId)) "Duplicate ticket ID '$ticketId' in derived label '$labelName'."
                $normalizedIds.Add($ticketId)
                if (-not $script:DerivedTicketLabels.ContainsKey($ticketId)) {
                    $script:DerivedTicketLabels[$ticketId] = [Collections.Generic.List[string]]::new()
                }
                $script:DerivedTicketLabels[$ticketId].Add($labelName)
            }
            $derivedList.Add(@{
                name = $labelName
                ticket_ids = [string[]] $normalizedIds.ToArray()
                source = [string] $entry['source']
                meaning = [string] $entry['meaning']
            })
        }
    }
    $script:DerivedLabels = @($derivedList.ToArray())
    foreach ($ticket in $script:Tickets.Values) {
        foreach ($name in @(Desired-Labels $ticket)) { Assert ($exactLabelNames.Contains($name)) "Missing registered label '$name' on $(Key $ticket)" }
        Assert ($script:MilestoneRegistry.ContainsKey((Milestone-Key $ticket))) "Unknown milestone on $(Key $ticket)"
        foreach ($field in $script:ProjectRegistry.fields | Where-Object { $_.kind -notlike 'NATIVE_*' }) {
            $value = Desired-Field $field $ticket
            if ($field.kind -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
                foreach ($option in @($value)) { Assert ($option -cin $field.options) "Unknown project value '$option' for $($field.name) on $(Key $ticket)" }
            }
            if ($field.kind -eq 'NUMBER') { Assert ($null -ne $value -and [double] $value -gt 0) "Invalid effort for $(Key $ticket)" }
        }
    }
    # Kahn order includes immediate containment and hard dependencies, including cross-repository edges.
    $remaining = @{}; $complete = @{}; $script:Order = [Collections.Generic.List[object]]::new()
    foreach ($ticket in $script:Tickets.Values) { $remaining[(Key $ticket)] = $ticket }
    while ($remaining.Count -gt 0) {
        $ready = @($remaining.Keys | Sort-Object | Where-Object {
            $ticket = $remaining[$_]
            $references = @($ticket.depends_on) + @($ticket.parent | Where-Object { $_ })
            @($references | Where-Object { -not $complete.ContainsKey((Key (Resolve-Ticket $_ $ticket))) }).Count -eq 0
        })
        Assert ($ready.Count -gt 0) "Combined parent/dependency graph has a cycle: $($remaining.Keys -join ', ')"
        foreach ($key in $ready) { $script:Order.Add($remaining[$key]); $complete[$key] = $true; $remaining.Remove($key) }
    }
    Select-PublicationScope
    Preflight-RenderedBodies
}
function Load-Ledger {
    if (Test-Path -LiteralPath $script:LedgerPath) {
        $script:Ledger = Read-Json $script:LedgerPath
        Assert ($script:Ledger.schema_version -eq 1 -and $script:Ledger.owner -ceq $Owner -and $script:Ledger.project_number -eq $ProjectNumber) 'Ledger owner/project/schema mismatch.'
        Assert ($script:Ledger.tickets -is [Collections.IDictionary] -and $script:Ledger.operations -is [Collections.IDictionary]) 'Invalid ledger structure.'
    } else {
        $script:Ledger = @{ schema_version = 1; owner = $Owner; project_number = $ProjectNumber; created_at = [DateTimeOffset]::UtcNow.ToString('o'); tickets = @{}; operations = @{} }
    }
    if (-not $script:Ledger.ContainsKey('registries')) { $script:Ledger.registries = @{ labels = @{}; milestones = @{} } }
}
function Refresh-Inventory {
    foreach ($repository in @('asciiwall', 'asciiwall-library')) {
        $script:Inventory[$repository] = @(Read-Pages "repos/$Owner/$repository/issues?state=all" | Where-Object { -not $_.Contains('pull_request') })
    }
}
function Find-Identity($Ticket) {
    $key = Key $Ticket; $marker = Marker $Ticket
    $known = $script:Ledger.tickets[$key]
    $candidates = @($script:Inventory[$Ticket.repository] | Where-Object {
        $body = [string] $_.body
        $id = [regex]::Escape($Ticket.id)
        $repository = [regex]::Escape($Ticket.repository)
        $metadata = $body -match ("(?m)^- (?:\*\*)?(?:Ticket ID|Ticket):(?:\*\*)?\s*(?:\x60|\*\*)?" + $id + "(?:\x60|\*\*)?\s*$") -and $body -match ("(?m)^- (?:\*\*)?Repository:(?:\*\*)?\s*(?:\x60|\*\*)?" + $repository + "(?:\x60|\*\*)?(?:\s+\([^)\r\n]*\))?\s*$")
        $body.Contains($marker) -or ($_.title -ceq $Ticket.title -and $metadata) -or ($null -ne $known -and $known.id -eq $_.id)
    })
    Assert ($candidates.Count -le 1) "Ambiguous existing issues for ${key}: $(@($candidates | ForEach-Object html_url) -join ', ')"
    if ($candidates.Count -eq 0) {
        Assert (-not $script:Ledger.tickets.ContainsKey($key) -or -not $script:Ledger.tickets[$key].ContainsKey('id')) "Ledger issue for $key is missing from exhaustive open/closed inventory; refusing recreation."
        return $null
    }
    $identity = $candidates[0]
    $foreignMarkers = @([regex]::Matches([string] $identity.body, '<!-- asciiwall-backlog:v1 [^>]+ -->') | ForEach-Object Value | Where-Object { $_ -cne $marker })
    Assert ($foreignMarkers.Count -eq 0) "Conflicting stable marker on candidate $($identity.html_url): $($foreignMarkers -join ', ')"
    Assert ($null -ne $identity.id -and $identity.node_id -and $identity.number -gt 0 -and $identity.html_url -ceq "https://github.com/$Owner/$($Ticket.repository)/issues/$($identity.number)") "Incomplete or foreign identity for $key"
    $old = $script:Ledger.tickets[$key]
    if ($null -ne $old -and $old.ContainsKey('id')) {
        Assert ($old.id -eq $identity.id -and $old.node_id -ceq $identity.node_id -and $old.number -eq $identity.number -and $old.url -ceq $identity.html_url) "Ledger identity drift for $key"
    }
    $script:Identities[$key] = $identity
    if ($Apply) {
        if ($null -eq $old) { $old = @{}; $script:Ledger.tickets[$key] = $old }
        $old.id = $identity.id; $old.node_id = $identity.node_id; $old.number = $identity.number; $old.url = $identity.html_url
        $old.marker = $marker; $old.source_sha256 = $Ticket._source_hash; $old.creation_state = 'identified'
        Save-Ledger
        Set-Reconciled "create:$key"
    }
    return $identity
}
function Render-Body($Ticket, [switch] $Initial) {
    $body = $Ticket._body
    $body = $body.Replace("`r`n", "`n").Replace("`r", "`n")
    $body = [regex]::Replace($body, '(?m)^- (?:\*\*)?Status:(?:\*\*)?.*?(?<effort>\s*;\s*estimated effort:.*)?$', [Text.RegularExpressions.MatchEvaluator] {
        param($match)
        "- Status: $($Ticket.status) — proposed, unimplemented; published for planning, not implementation evidence$($match.Groups['effort'].Value)"
    })
    $body = [regex]::Replace($body, '(?m)^- (?:\*\*)?Ticket(?: ID)?:(?:\*\*)?.*$', "- Ticket ID: $($Ticket.id)")
    $body = [regex]::Replace($body, '(?m)^- (?:\*\*)?Repository:(?:\*\*)?.*$', "- Repository: $($Ticket.repository)")
    $body = [regex]::Replace($body, '(?im)^- (?:\*\*)?Publication(?: state)?:(?:\*\*)?.*$', '- Publication: published GitHub issue; durable identity and reconciliation evidence are maintained in the external publication ledger.')
    $body = [regex]::Replace($body, '(?im)^- (?:\*\*)?(?:Planned GitHub )?Milestone(?:\s*\([^)]*\))?(?:\s+target)?:(?:\*\*)?.*$|^- (?:\*\*)?GitHub milestone(?:\*\*)?(?:\s*\([^)]*\))?(?:\s+target)?(?:\s*\([^)]*\))?:(?:\*\*)?.*$', "- GitHub milestone: $($script:MilestoneRegistry[(Milestone-Key $Ticket)].title) (native milestone; implementation remains unimplemented).")
    $projectState = if ($Initial) { 'native issue-item reconciliation pending' } else { 'actual issue item; verified identity is recorded in the external publication ledger' }
    $projectUrl = "https://github.com/orgs/$Owner/projects/$ProjectNumber ($projectState)."
    $body = [regex]::Replace($body, '(?im)^- (?:\*\*)?GitHub fields:(?:\*\*)?.*$|^- (?:\*\*)?Completion status:(?:\*\*)?.*local-only.*$', '- GitHub fields and issue identity are reconciled through the external publication ledger; this ticket remains Backlog, draft/unimplemented.')
    $body = [regex]::Replace($body, '(?im)^- (?:\*\*)?Completion:(?:\*\*)? Backlog; local-only;.*$', '- Completion: Backlog; no implementation or test pass claimed.')
    $body = [regex]::Replace($body, '(?im)^- (?:\*\*)?GitHub Project(?: \(reference only\))?:(?:\*\*)?.*$|^- (?:\*\*)?Project:(?:\*\*)?.*$', "- GitHub Project: $projectUrl")
    $body = [regex]::Replace($body, '(?im)^- GitHub milestone \(planned(?: local value)?\):.*$|^- GitHub milestone target:.*$', "- GitHub milestone: $($script:MilestoneRegistry[(Milestone-Key $Ticket)].title) (native milestone; implementation remains unimplemented).")
    $body = $body.Replace('No remote issue, project item or release is created by this ticket.', 'Publication creates planning records; it does not create releases or claim implementation.')
    $body = $body.Replace('No remote issues/projects/releases are created by this local backlog.', 'Publication creates planning records; it does not create releases or claim implementation.')
    $body = Convert-UnfencedLinks $body $Ticket -Initial:$Initial
    Assert ($body -notmatch '(?i)https://github\.com/[^\s/]+/[^\s/]+/issues/0(?:[^\d]|$)') "Placeholder issue URL in $(Key $Ticket)"
    # No implementation checkboxes or acceptance text are changed by publication.
    $rendered = (Marker $Ticket) + "`n<!-- source-sha256:$($Ticket._source_hash) -->`n" + $body.TrimEnd() + "`n"
    Assert-BodyLimit $rendered $Ticket 'Rendered'
    return $rendered
}
function Convert-UnfencedLinks([string] $Body, $Ticket, [switch] $Initial) {
    $insideFence = $false
    $lines = foreach ($line in ($Body -split "`n")) {
        if ($line -match '^\s*(```|~~~)') { $insideFence = -not $insideFence; $line; continue }
        if ($insideFence) { $line; continue }
        if (-not $line.Contains('](')) { $line; continue }
        [regex]::Replace($line, '(?<open>!?\[[^\]\r\n]*\]\()(?<target><[^>\r\n]+>|[^\s)]+)(?<title>\s+"[^"\r\n]*")?(?<close>\))', [Text.RegularExpressions.MatchEvaluator] {
            param($match)
            $target = $match.Groups['target'].Value.TrimStart('<').TrimEnd('>')
            if ($target -match '^(?:https?://|mailto:|#)') { return $match.Value }
            $decoded = [Uri]::UnescapeDataString($target)
            $fragment = ''; $path = $decoded
            if ($decoded.Contains('#')) { $at = $decoded.IndexOf('#'); $path = $decoded.Substring(0, $at); $fragment = $decoded.Substring($at) }
            if ($path -match '(?:^|/)(docs/.+)$') {
                $relative = ($Matches[1] -split '/' | ForEach-Object { Escape-Segment $_ }) -join '/'
                $refPath = ($script:SourceSha -split '/' | ForEach-Object { Escape-Segment $_ }) -join '/'
                return $match.Groups['open'].Value + "https://github.com/$Owner/asciiwall/blob/$refPath/$relative$fragment" + $match.Groups['title'].Value + $match.Groups['close'].Value
            }
            if ($path -match '(?:^|/)(coverage/[^/]+\.json)$') {
                $reference = $Matches[1]
                $label = $match.Groups['open'].Value -replace '^!?\[', '' -replace '\]\($', ''
                return $label + ' (`external backlog: ' + $reference + '`)'
            }
            if ($path -match '(?:^|/)([^/]+)/(?:CONTENT\.md|issue\.json)$' -or $path -match '^(?:CONTENT\.md|issue\.json)$') {
                $id = if ($Matches[1]) { $Matches[1] } else { $Ticket.id }
                $resolvedPath = [IO.Path]::GetFullPath((Join-Path $Ticket._directory $path))
                $directory = [IO.Path]::GetDirectoryName($resolvedPath)
                $destinations = if ($null -ne $script:TicketsByDirectory -and $script:TicketsByDirectory.ContainsKey($directory)) { $script:TicketsByDirectory[$directory] } else { @() }
                $destination = if ($destinations.Count -eq 1) { $destinations[0] } else { Resolve-Ticket $id $Ticket }
                $identity = $script:Identities[(Key $destination)]
                if ($null -eq $identity -and -not $script:PublicationKeys.Contains((Key $destination))) {
                    return ($match.Value -replace '\]\([^)]*\)$', ']' -replace '^!?\[', '' -replace '\]$', '') + ' (local backlog; not published in this batch)'
                }
                if ($null -eq $identity -and $Initial) { return ($match.Value -replace '\]\([^)]*\)$', ']' -replace '^!?\[', '' -replace '\]$', '') }
                Assert ($null -ne $identity) "Cannot render unpublished ticket $(Key $destination) in $(Key $Ticket)"
                return $match.Groups['open'].Value + $identity.html_url + $fragment + $match.Groups['title'].Value + $match.Groups['close'].Value
            }
            throw "Unsupported local Markdown link '$target' in $(Key $Ticket); cannot publish a broken relative link."
        })
    }
    return $lines -join "`n"
}

function Ensure-Registries {
    foreach ($repository in @('asciiwall', 'asciiwall-library')) {
        $labels = @(Read-Pages "repos/$Owner/$repository/labels")
        foreach ($desired in $script:LabelRegistry) {
            $matches = @($labels | Where-Object { $_.name -ieq $desired.name })
            Assert ($matches.Count -le 1) "Ambiguous label '$($desired.name)' in $repository"
            $op = "label:$repository/$($desired.name)"
            if ($matches.Count -eq 0) {
                $remoteLabel = Mutate $op 'POST' "repos/$Owner/$repository/labels" @{ name = $desired.name; color = $desired.color; description = $desired.description }
            } elseif ($matches[0].name -cne $desired.name -or $matches[0].color -ine $desired.color -or [string] $matches[0].description -cne [string] $desired.description) {
                $remoteLabel = Mutate $op 'PATCH' "repos/$Owner/$repository/labels/$(Escape-Segment $matches[0].name)" @{ new_name = $desired.name; color = $desired.color; description = $desired.description }
            } else { $remoteLabel = $matches[0]; Set-Reconciled $op }
            $script:Ledger.registries.labels["$repository/$($desired.name)"] = @{ state = 'submitted'; desired = $desired; observed = $remoteLabel }
            Save-Ledger
        }
        $milestones = @(Read-Pages "repos/$Owner/$repository/milestones?state=all")
        foreach ($key in @($script:MilestoneRegistry.Keys | Sort-Object)) {
            if (-not $script:UsedMilestones.Contains($key)) { continue }
            $desired = $script:MilestoneRegistry[$key]
            if ($desired.repository -ne $repository) { continue }
            $matches = @($milestones | Where-Object { $_.title -ceq $desired.title })
            Assert ($matches.Count -le 1) "Ambiguous milestone '$($desired.title)' in $repository"
            if (-not $script:ManagedMilestones.Contains($key)) {
                Assert ($matches.Count -eq 1) "Required ancestor milestone '$key' does not exist; refusing to create a milestone outside the requested scope."
                $script:Milestones[$key] = $matches[0]
                continue
            }
            $op = "milestone:$key"
            if ($matches.Count -eq 0) {
                $remote = Mutate $op 'POST' "repos/$Owner/$repository/milestones" @{ title = $desired.title; description = $desired.description }
            } else {
                $remote = $matches[0]
                if ([string] $remote.description -cne [string] $desired.description) {
                    $remote = Mutate $op 'PATCH' "repos/$Owner/$repository/milestones/$($remote.number)" @{ title = $desired.title; description = $desired.description }
                } else { Set-Reconciled $op }
            }
            Assert ($null -ne $remote.number -and $remote.node_id) "Milestone identity missing: $key"
            $script:Milestones[$key] = $remote
            $script:Ledger.registries.milestones[$key] = @{ state = 'submitted'; number = $remote.number; node_id = $remote.node_id; title = $remote.title; description = $remote.description; due_on = $remote.due_on; remote_state = $remote.state }
            Save-Ledger
        }
    }
}
function Input-TypeName($Type) {
    while ($null -ne $Type -and -not $Type.name) { $Type = $Type.ofType }
    if ($null -ne $Type) { return $Type.name }
}
function Read-SchemaType([string] $Name) {
    if (-not $script:Schema.ContainsKey($Name)) {
        $typeSelection = 'kind name'
        for ($depth = 0; $depth -lt 4; $depth++) { $typeSelection = 'kind name ofType{' + $typeSelection + '}' }
        $query = 'query($name:String!){__type(name:$name){name kind inputFields{name type{' + $typeSelection + '}} fields{name}}}'
        $data = Graph-Read $query @{ name = $Name }
        $script:Schema[$Name] = $data.__type
    }
    $script:Schema[$Name]
}
function Require-ProjectSchema {
    $value = Read-SchemaType 'ProjectV2FieldValue'
    Assert ($null -ne $value -and 'multiSelectOptionIds' -in @($value.inputFields.name)) 'GitHub schema lacks native multiSelectOptionIds; Platform cannot be represented faithfully.'
    $multi = Read-SchemaType 'ProjectV2MultiSelectField'
    Assert ($null -ne $multi -and 'multiSelectOptions' -in @($multi.fields.name)) 'GitHub schema lacks native project multi-select fields.'
    foreach ($inputName in @('CreateProjectV2FieldInput', 'UpdateProjectV2FieldInput')) {
        $inputType = Read-SchemaType $inputName
        foreach ($property in @('singleSelectOptions', 'multiSelectOptions')) {
            $propertyType = @($inputType.inputFields | Where-Object name -eq $property)
            Assert ($propertyType.Count -eq 1) "GitHub schema lacks $inputName.$property; no lossy text fallback is permitted."
            $optionName = Input-TypeName $propertyType[0].type
            $option = Read-SchemaType $optionName
            if ($inputName -eq 'UpdateProjectV2FieldInput') {
                Assert ('id' -in @($option.inputFields.name)) "Cannot preserve existing option IDs: $optionName does not accept id."
            }
        }
    }
}
function Refresh-Project {
    $script:Fields = @{}
    $cursor = $null
    do {
        $data = Graph-Read 'query($owner:String!,$number:Int!,$cursor:String){organization(login:$owner){projectV2(number:$number){id number title fields(first:100,after:$cursor){nodes{__typename ... on ProjectV2Field{id name dataType} ... on ProjectV2SingleSelectField{id name dataType options{id name color description}} ... on ProjectV2MultiSelectField{id name dataType multiSelectOptions{id name color description}} ... on ProjectV2IterationField{id name dataType}} pageInfo{hasNextPage endCursor}}}}}' @{ owner = $Owner; number = $ProjectNumber; cursor = $cursor }
        $project = $data.organization.projectV2
        Assert ($null -ne $project) "Cannot read organization Project $ProjectNumber"
        $script:Project = $project
        foreach ($field in $project.fields.nodes) {
            Assert ($field.id -and $field.name -and -not $script:Fields.ContainsKey($field.name)) "Unrecognized or duplicate project field: $(Json $field)"
            $script:Fields[$field.name] = $field
        }
        $cursor = $project.fields.pageInfo.endCursor
    } while ($project.fields.pageInfo.hasNextPage)
    if ($script:ProjectRegistry.id) { Assert ($project.id -ceq $script:ProjectRegistry.id) 'Remote project identity differs from source registry.' }
}
function Field-Options($Field) {
    if ($Field.dataType -eq 'MULTI_SELECT') { return @($Field.multiSelectOptions) }
    return @($Field.options)
}
function Ensure-Project {
    Refresh-Project
    if ($script:Project.title -cne $script:ProjectRegistry.desired_title) {
        [void] (Graph-Mutate 'project:title' 'mutation($input:UpdateProjectV2Input!){updateProjectV2(input:$input){projectV2{id title}}}' @{ input = @{ projectId = $script:Project.id; title = $script:ProjectRegistry.desired_title } })
    } else { Set-Reconciled 'project:title' }
    $linked = [Collections.Generic.List[string]]::new(); $cursor = $null
    do {
        $data = Graph-Read 'query($id:ID!,$cursor:String){node(id:$id){... on ProjectV2{repositories(first:100,after:$cursor){nodes{id nameWithOwner} pageInfo{hasNextPage endCursor}}}}}' @{ id = $script:Project.id; cursor = $cursor }
        foreach ($repo in $data.node.repositories.nodes) { $linked.Add($repo.nameWithOwner) }
        $cursor = $data.node.repositories.pageInfo.endCursor
    } while ($data.node.repositories.pageInfo.hasNextPage)
    foreach ($repository in @('asciiwall', 'asciiwall-library')) {
        $op = "project:repository:$repository"
        if ("$Owner/$repository" -cnotin $linked) {
            $repo = Read-Api "repos/$Owner/$repository"
            [void] (Graph-Mutate $op 'mutation($input:LinkProjectV2ToRepositoryInput!){linkProjectV2ToRepository(input:$input){repository{id}}}' @{ input = @{ projectId = $script:Project.id; repositoryId = $repo.node_id } })
        } else { Set-Reconciled $op }
    }
    foreach ($desired in $script:ProjectRegistry.fields) {
        $field = $script:Fields[$desired.name]; $op = "project:field:$($desired.name)"
        if ($desired.kind -like 'NATIVE_*') {
            $expected = $desired.kind.Substring(7)
            Assert ($null -ne $field -and $field.dataType -eq $expected) "Required native field $($desired.name) is missing or shadowed."
            continue
        }
        if ($null -eq $field) {
            $inputValue = @{ projectId = $script:Project.id; name = $desired.name; dataType = $desired.kind }
            if ($desired.kind -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
                $property = if ($desired.kind -eq 'MULTI_SELECT') { 'multiSelectOptions' } else { 'singleSelectOptions' }
                $inputValue[$property] = @($desired.options | ForEach-Object { @{ name = $_; color = 'GRAY'; description = '' } })
            }
            [void] (Graph-Mutate $op 'mutation($input:CreateProjectV2FieldInput!){createProjectV2Field(input:$input){projectV2Field{... on ProjectV2Field{id} ... on ProjectV2SingleSelectField{id} ... on ProjectV2MultiSelectField{id}}}}' @{ input = $inputValue })
            Refresh-Project
            $field = $script:Fields[$desired.name]
            Assert ($null -ne $field) "New field absent after creation: $($desired.name)"
        }
        Assert ($field.dataType -eq $desired.kind) "Project field '$($desired.name)' has incompatible native type $($field.dataType), expected $($desired.kind)."
        if ($desired.kind -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
            $options = @(Field-Options $field)
            Assert (@($options.name | Sort-Object -Unique).Count -eq $options.Count) "Duplicate option names in $($desired.name)"
            $missing = @($desired.options | Where-Object { $_ -cnotin @($options.name) })
            if ($missing.Count -gt 0) {
                Assert (($options.Count + $missing.Count) -le 50) "Preserving existing options would exceed GitHub's 50-option limit in $($desired.name). Manual non-destructive project migration required."
                $merged = @($options | ForEach-Object { @{ id = $_.id; name = $_.name; color = $_.color; description = [string] $_.description } }) + @($missing | ForEach-Object { @{ name = $_; color = 'GRAY'; description = '' } })
                $property = if ($desired.kind -eq 'MULTI_SELECT') { 'multiSelectOptions' } else { 'singleSelectOptions' }
                $inputValue = @{ fieldId = $field.id; $property = $merged }
                [void] (Graph-Mutate $op 'mutation($input:UpdateProjectV2FieldInput!){updateProjectV2Field(input:$input){projectV2Field{... on ProjectV2Field{id} ... on ProjectV2SingleSelectField{id} ... on ProjectV2MultiSelectField{id}}}}' @{ input = $inputValue })
                Refresh-Project
                $after = @(Field-Options $script:Fields[$desired.name])
                foreach ($option in $options) {
                    Assert (@($after | Where-Object { $_.id -ceq $option.id -and $_.name -ceq $option.name -and $_.color -ceq $option.color -and [string] $_.description -ceq [string] $option.description }).Count -eq 1) "Option identity or user metadata changed in $($desired.name): $($option.id)"
                }
            } else { Set-Reconciled $op }
        } else { Set-Reconciled $op }
    }
    $derivedSummary = @($script:DerivedLabels | ForEach-Object { @{ name = $_.name; ticket_ids = [string[]] @($_.ticket_ids); source = $_.source; meaning = $_.meaning } })
    $script:Ledger.project = @{ id = $script:Project.id; number = $ProjectNumber; state = 'submitted'; title = $script:ProjectRegistry.desired_title; fields = $script:Fields; repositories = $script:ProjectRegistry.repositories; derived_labels = $derivedSummary }
    Save-Ledger
}
function Ensure-Issues {
    Refresh-Inventory
    foreach ($ticket in $script:Order) {
        $key = Key $ticket
        $identity = Find-Identity $ticket
        if ($null -eq $identity) {
            $payload = @{ title = $ticket.title; body = Render-Body $ticket -Initial; labels = @(Desired-Labels $ticket); milestone = $script:Milestones[(Milestone-Key $ticket)].number; type = Native-Type $ticket }
            $created = Mutate "create:$key" 'POST' "repos/$Owner/$($ticket.repository)/issues" $payload
            Assert ($created.id -and $created.node_id -and $created.number -gt 0) "Creation returned incomplete identity for $key; stop and rediscover before retrying."
            $script:Inventory[$ticket.repository] += $created
            $identity = Find-Identity $ticket
        }
        $script:Identities[$key] = $identity
    }
    foreach ($ticket in $script:Order) {
        $key = Key $ticket; $identity = $script:Identities[$key]
        $remote = Read-Api "repos/$Owner/$($ticket.repository)/issues/$($identity.number)"
        $ticketLedger = $script:Ledger.tickets[$key]
        $priorDesired = if ($null -ne $ticketLedger -and $ticketLedger.Contains('desired')) { $ticketLedger['desired'] } else { $null }
        $priorVerification = if ($null -ne $ticketLedger -and $ticketLedger.Contains('verification')) { $ticketLedger['verification'] } else { $null }
        $createOp = if ($script:Ledger.operations.ContainsKey("create:$key")) { $script:Ledger.operations["create:$key"] } else { $null }
        $metadataOp = if ($script:Ledger.operations.ContainsKey("metadata:$key")) { $script:Ledger.operations["metadata:$key"] } else { $null }

        $body = Render-Body $ticket
        $desiredBodySha = Hash $body
        $unmanaged = @($remote.labels | ForEach-Object name | Where-Object { -not [string]::IsNullOrEmpty($_) -and -not (Is-ManagedLabel $_) })
        $labels = @(@(Desired-Labels $ticket) + $unmanaged | Sort-Object -Unique)
        $desired = @{ title = $ticket.title; body = $body; labels = $labels; milestone = $script:Milestones[(Milestone-Key $ticket)].number; type = Native-Type $ticket }
        $op = "metadata:$key"
        $needsPatch = $remote.title -cne $desired.title -or [string] $remote.body -cne $body -or -not (Same-Set @($remote.labels.name) $labels) -or $remote.milestone.number -ne $desired.milestone -or $remote.type.name -cne $desired.type

        if ($needsPatch) {
            Assert ([string] $remote.state -ine 'closed') "Refusing to apply Backlog planning metadata to already closed issue $key ($($remote.html_url)); closed issues must not be overwritten or reopened."

            $actualBody = [string] $remote.body
            if ($actualBody -cne $body) {
                $actualBodySha = Hash $actualBody
                $actualBodyLfSha = Hash ($actualBody.Replace("`r`n", "`n").Replace("`r", "`n"))
                $knownManagedHashes = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                [void] $knownManagedHashes.Add($desiredBodySha)
                if ($null -ne $priorDesired -and $priorDesired -is [Collections.IDictionary] -and -not [string]::IsNullOrWhiteSpace([string] $priorDesired['body_sha256'])) {
                    [void] $knownManagedHashes.Add([string] $priorDesired['body_sha256'])
                }
                if ($null -ne $priorVerification -and $priorVerification -is [Collections.IDictionary] -and $priorVerification['state'] -ceq 'verified' -and -not [string]::IsNullOrWhiteSpace([string] $priorVerification['body_sha256'])) {
                    [void] $knownManagedHashes.Add([string] $priorVerification['body_sha256'])
                }
                if ($null -ne $createOp -and $createOp -is [Collections.IDictionary] -and $createOp.Contains('payload') -and $createOp['payload'] -is [Collections.IDictionary] -and $createOp['payload'].Contains('body') -and $null -ne $createOp['payload']['body']) {
                    [void] $knownManagedHashes.Add((Hash ([string] $createOp['payload']['body'])))
                }
                if ($null -ne $metadataOp -and $metadataOp -is [Collections.IDictionary] -and $metadataOp['state'] -cin @('succeeded', 'reconciled', 'verified') -and $metadataOp.Contains('payload') -and $metadataOp['payload'] -is [Collections.IDictionary] -and $metadataOp['payload'].Contains('body') -and $null -ne $metadataOp['payload']['body']) {
                    [void] $knownManagedHashes.Add((Hash ([string] $metadataOp['payload']['body'])))
                }
                $rawSource = [string] $ticket._body
                $rawSourceSha = Hash $rawSource
                $rawSourceLfSha = Hash ($rawSource.Replace("`r`n", "`n").Replace("`r", "`n"))
                [void] $knownManagedHashes.Add($rawSourceSha)
                [void] $knownManagedHashes.Add($rawSourceLfSha)

                $matchesKnown = $knownManagedHashes.Contains($actualBodySha) -or ($actualBodyLfSha -ceq $rawSourceLfSha)
                Assert $matchesKnown "Body conflict for $key ($($remote.html_url)): remote body hash $actualBodySha differs from all known managed bodies (prior desired, initial create payload, current rendered desired, and current raw source template); refusing to overwrite remote edits/evidence."
            }

            $remote = Mutate $op 'PATCH' "repos/$Owner/$($ticket.repository)/issues/$($identity.number)" $desired
        } else { Set-Reconciled $op }

        $script:Identities[$key] = $remote
        $script:Ledger.tickets[$key].desired = @{ body_sha256 = $desiredBodySha; title = $ticket.title; labels = $labels; milestone = $desired.milestone; native_type = $desired.type; source_sha256 = $ticket._source_hash }
        $script:Ledger.tickets[$key].metadata_state = 'submitted'
        Save-Ledger
    }
}
function Ensure-Relationships {
    foreach ($ticket in $script:Order) {
        $key = Key $ticket; $identity = $script:Identities[$key]
        $endpoint = "repos/$Owner/$($ticket.repository)/issues/$($identity.number)"
        $desiredParent = if ($ticket.parent) { $script:Identities[(Key (Resolve-Ticket $ticket.parent $ticket))] } else { $null }
        $expected = @($ticket.depends_on | ForEach-Object { $script:Identities[(Key (Resolve-Ticket $_ $ticket))] })
        $script:Ledger.tickets[$key].relationships = @{ state = 'planned'; parent_id = $(if ($desiredParent) { $desiredParent.id } else { $null }); blocker_ids = @($expected.id); dependent_endpoint = $endpoint }
        Save-Ledger
        $parent = Read-Api "$endpoint/parent" -AllowNotFound
        $op = "parent:$key"
        if ($null -ne $parent) {
            Assert ($null -ne $desiredParent -and $parent.id -eq $desiredParent.id) "Conflicting native parent for ${key}: $($parent.html_url); refusing replace_parent or deletion."
            Set-Reconciled $op
        } elseif ($null -ne $desiredParent) {
            $parentTicket = Resolve-Ticket $ticket.parent $ticket
            [void] (Mutate $op 'POST' "repos/$Owner/$($parentTicket.repository)/issues/$($desiredParent.number)/sub_issues" @{ sub_issue_id = $identity.id })
        }
        $blockers = @(Read-Pages "$endpoint/dependencies/blocked_by")
        $unexpected = @($blockers | Where-Object { $_.id -notin @($expected.id) })
        Assert ($unexpected.Count -eq 0) "Unexpected native blockers on $key; preserving them and stopping: $(@($unexpected.html_url) -join ', ')"
        foreach ($blocker in $expected) {
            $op = "blocker:$key/$($blocker.id)"
            if ($blocker.id -in @($blockers.id)) { Set-Reconciled $op }
            else { [void] (Mutate $op 'POST' "$endpoint/dependencies/blocked_by" @{ issue_id = $blocker.id }) }
        }
        if ($Apply) {
            $script:Ledger.tickets[$key].relationships = @{ state = 'submitted'; parent_id = $(if ($desiredParent) { $desiredParent.id } else { $null }); blocker_ids = @($expected.id) }
            Save-Ledger
        }
    }
}
function Refresh-Items {
    $script:ProjectItems = @{}; $cursor = $null
    do {
        $data = Graph-Read 'query($id:ID!,$cursor:String){node(id:$id){... on ProjectV2{items(first:100,after:$cursor){nodes{id content{__typename ... on Issue{id number repository{nameWithOwner}}}} pageInfo{hasNextPage endCursor}}}}}' @{ id = $script:Project.id; cursor = $cursor }
        foreach ($item in $data.node.items.nodes) {
            if ($item.content.__typename -ne 'Issue') { continue }
            $issueId = $item.content.id
            Assert (-not $script:ProjectItems.ContainsKey($issueId)) "Duplicate actual-issue project items for $issueId"
            $script:ProjectItems[$issueId] = $item
        }
        $cursor = $data.node.items.pageInfo.endCursor
    } while ($data.node.items.pageInfo.hasNextPage)
}
function Read-ItemValues([string] $ItemId, [string] $IssueNodeId) {
    $values = @{}; $cursor = $null
    do {
        $data = Graph-Read 'query($id:ID!,$cursor:String){node(id:$id){... on ProjectV2Item{id content{... on Issue{id}} fieldValues(first:100,after:$cursor){nodes{__typename ... on ProjectV2ItemFieldTextValue{text field{... on ProjectV2FieldCommon{id name}}} ... on ProjectV2ItemFieldNumberValue{number field{... on ProjectV2FieldCommon{id name}}} ... on ProjectV2ItemFieldSingleSelectValue{optionId name field{... on ProjectV2FieldCommon{id name}}} ... on ProjectV2ItemFieldMultiSelectValue{options{id name} field{... on ProjectV2FieldCommon{id name}}} ... on ProjectV2ItemFieldRepositoryValue{repository{id nameWithOwner} field{... on ProjectV2FieldCommon{id name}}} ... on ProjectV2ItemFieldMilestoneValue{milestone{number title} field{... on ProjectV2FieldCommon{id name}}} ... on ProjectV2ItemFieldUserValue{users(first:100){nodes{login} pageInfo{hasNextPage}} field{... on ProjectV2FieldCommon{id name}}}} pageInfo{hasNextPage endCursor}}}}}' @{ id = $ItemId; cursor = $cursor }
        Assert ($data.node.content.id -ceq $IssueNodeId) "Project item $ItemId does not contain the expected actual issue $IssueNodeId."
        foreach ($value in $data.node.fieldValues.nodes) {
            if ($value.field.name) {
                Assert (-not $values.ContainsKey($value.field.name)) "Duplicate project value $($value.field.name) on $ItemId"
                $values[$value.field.name] = $value
                if ($value.__typename -eq 'ProjectV2ItemFieldUserValue') { Assert (-not $value.users.pageInfo.hasNextPage) "More than 100 assignees on $ItemId; cannot verify exhaustively." }
            }
        }
        $cursor = $data.node.fieldValues.pageInfo.endCursor
    } while ($data.node.fieldValues.pageInfo.hasNextPage)
    return $values
}
function Field-Payload($Desired, $Field, $Ticket) {
    $value = Desired-Field $Desired $Ticket
    switch ($Desired.kind) {
        'SINGLE_SELECT' {
            $option = @((Field-Options $Field) | Where-Object { $_.name -ceq $value })
            Assert ($option.Count -eq 1) "Missing/ambiguous project option $($Desired.name)=$value"
            return @{ singleSelectOptionId = $option[0].id }
        }
        'MULTI_SELECT' {
            $ids = @(); foreach ($name in @($value)) {
                $option = @((Field-Options $Field) | Where-Object { $_.name -ceq $name })
                Assert ($option.Count -eq 1) "Missing/ambiguous multi-select option $($Desired.name)=$name"
                $ids += $option[0].id
            }
            return @{ multiSelectOptionIds = $ids }
        }
        'TEXT' { return @{ text = [string] $value } }
        'NUMBER' { return @{ number = [double] $value } }
    }
    throw "Cannot set native project field $($Desired.name)"
}
function Field-Matches($Desired, $Actual) {
    if ($null -eq $Actual) { return $false }
    if ($Desired.ContainsKey('singleSelectOptionId')) { return $Actual.optionId -ceq $Desired.singleSelectOptionId }
    if ($Desired.ContainsKey('multiSelectOptionIds')) { return Same-Set @($Actual.options.id) @($Desired.multiSelectOptionIds) }
    if ($Desired.ContainsKey('text')) { return [string] $Actual.text -ceq [string] $Desired.text }
    if ($Desired.ContainsKey('number')) { return $null -ne $Actual.number -and [double] $Actual.number -eq [double] $Desired.number }
    return $false
}
function Get-RecordedVerifiedStatus($TicketLedger) {
    if ($null -eq $TicketLedger -or $TicketLedger -isnot [Collections.IDictionary]) { return $null }
    if ($TicketLedger.Contains('verification')) {
        $verification = $TicketLedger['verification']
        if ($null -ne $verification -and $verification -is [Collections.IDictionary] -and $verification['state'] -ceq 'verified' -and $verification.Contains('fields')) {
            $fields = $verification['fields']
            if ($null -ne $fields -and $fields -is [Collections.IDictionary] -and $fields.Contains('Status')) {
                return $fields['Status']
            }
        }
    }
    if ($TicketLedger.Contains('project')) {
        $project = $TicketLedger['project']
        if ($null -ne $project -and $project -is [Collections.IDictionary] -and $project['state'] -ceq 'verified' -and $project.Contains('fields')) {
            $fields = $project['fields']
            if ($null -ne $fields -and $fields -is [Collections.IDictionary] -and $fields.Contains('Status')) {
                return $fields['Status']
            }
        }
    }
    return $null
}
function Status-Matches($Actual, $Verified) {
    if ($null -eq $Actual -and $null -eq $Verified) { return $true }
    if ($null -eq $Actual -or $null -eq $Verified) { return $false }
    $actualId = if ($Actual -is [Collections.IDictionary] -and $Actual.Contains('optionId')) { [string] $Actual['optionId'] } else { '' }
    $verifiedId = if ($Verified -is [Collections.IDictionary] -and $Verified.Contains('optionId') -and $Verified['optionId']) { [string] $Verified['optionId'] } elseif ($Verified -is [Collections.IDictionary] -and $Verified.Contains('singleSelectOptionId') -and $Verified['singleSelectOptionId']) { [string] $Verified['singleSelectOptionId'] } else { '' }
    if ($actualId -and $verifiedId) { return $actualId -ceq $verifiedId }
    $actualName = if ($Actual -is [Collections.IDictionary] -and $Actual.Contains('name')) { [string] $Actual['name'] } else { '' }
    $verifiedName = if ($Verified -is [Collections.IDictionary] -and $Verified.Contains('name')) { [string] $Verified['name'] } else { '' }
    if ($actualName -and $verifiedName) { return $actualName -ceq $verifiedName }
    return $false
}
function Ensure-Items {
    Refresh-Items
    $nonNativeFields = @($script:ProjectRegistry.fields | Where-Object { $_.kind -notlike 'NATIVE_*' })
    $statusMatches = @($nonNativeFields | Where-Object { $_.name -ceq 'Status' })
    Assert ($statusMatches.Count -eq 1) 'Project registry is missing required Status field.'
    $statusDesired = $statusMatches[0]
    $orderedFields = @($statusDesired) + @($nonNativeFields | Where-Object { $_.name -cne 'Status' })
    $statusField = $script:Fields['Status']
    Assert ($null -ne $statusField) 'Remote project is missing required Status field.'
    $todoOptions = @((Field-Options $statusField) | Where-Object { $_.name -ceq 'Todo' })

    foreach ($ticket in $script:Order) {
        $key = Key $ticket; $identity = $script:Identities[$key]; $op = "project:item:$key"
        $remote = $identity
        $item = $script:ProjectItems[$identity.node_id]
        if ($null -eq $item) {
            Assert ([string] $remote.state -ine 'closed') "Refusing to add already closed issue $key ($($remote.html_url)) to Project $ProjectNumber with Backlog planning metadata."
            $data = Graph-Mutate $op 'mutation($input:AddProjectV2ItemByIdInput!){addProjectV2ItemById(input:$input){item{id}}}' @{ input = @{ projectId = $script:Project.id; contentId = $identity.node_id } }
            $item = $data.addProjectV2ItemById.item
            Assert ([bool] $item.id) "Project item identity missing for $key"
            $script:ProjectItems[$identity.node_id] = $item
        } else { Set-Reconciled $op }
        $values = Read-ItemValues $item.id $identity.node_id

        $statusPayload = Field-Payload $statusDesired $statusField $ticket
        $actualStatus = $values['Status']
        $desiredStatusName = [string] (Desired-Field $statusDesired $ticket)
        if (-not (Field-Matches $statusPayload $actualStatus)) {
            $actualLabel = if ($null -ne $actualStatus -and $actualStatus.name) { [string] $actualStatus.name } elseif ($null -ne $actualStatus -and $actualStatus.optionId) { [string] $actualStatus.optionId } else { '<unset>' }
            Assert ([string] $remote.state -ine 'closed') "Refusing to set project Status to '$desiredStatusName' on already closed issue $key ($($remote.html_url)); current Status is '$actualLabel'."
            $verifiedStatus = Get-RecordedVerifiedStatus $script:Ledger.tickets[$key]
            if ($null -ne $verifiedStatus) {
                if (-not (Status-Matches $actualStatus $verifiedStatus)) {
                    $verifiedLabel = if ($verifiedStatus -is [Collections.IDictionary] -and $verifiedStatus['name']) { [string] $verifiedStatus['name'] } elseif ($verifiedStatus -is [Collections.IDictionary] -and $verifiedStatus['optionId']) { [string] $verifiedStatus['optionId'] } elseif ($verifiedStatus -is [Collections.IDictionary] -and $verifiedStatus['singleSelectOptionId']) { [string] $verifiedStatus['singleSelectOptionId'] } else { '<unset>' }
                    throw "Project Status conflict for $key ($($remote.html_url)): remote Status '$actualLabel' differs from recorded verified Status '$verifiedLabel' and desired Status '$desiredStatusName'; refusing to roll back manual progress."
                }
            } else {
                $actualName = if ($null -ne $actualStatus) { [string] $actualStatus.name } else { '' }
                $actualId = if ($null -ne $actualStatus) { [string] $actualStatus.optionId } else { '' }
                $isTodoById = $todoOptions.Count -eq 1 -and $actualId -ceq $todoOptions[0].id
                $isInitialUnconfiguredOrTodo = ($null -eq $actualStatus) -or ([string]::IsNullOrEmpty($actualName) -and [string]::IsNullOrEmpty($actualId)) -or ($actualName -ceq 'Todo') -or $isTodoById
                if (-not $isInitialUnconfiguredOrTodo) {
                    throw "Project Status conflict for $key ($($remote.html_url)): unverified remote Status '$actualLabel' is not initial unconfigured/default 'Todo' or desired Status '$desiredStatusName'; refusing to roll back manual progress."
                }
            }
        }

        $desiredValues = @{}
        foreach ($desired in $orderedFields) {
            $field = $script:Fields[$desired.name]
            $payload = Field-Payload $desired $field $ticket
            $desiredValues[$desired.name] = $payload
            $op = "project:value:$key/$($desired.name)"
            if (-not (Field-Matches $payload $values[$desired.name])) {
                [void] (Graph-Mutate $op 'mutation($input:UpdateProjectV2ItemFieldValueInput!){updateProjectV2ItemFieldValue(input:$input){projectV2Item{id}}}' @{ input = @{ projectId = $script:Project.id; itemId = $item.id; fieldId = $field.id; value = $payload } })
            } else { Set-Reconciled $op }
        }
        $script:Ledger.tickets[$key].project = @{ item_id = $item.id; state = 'submitted'; fields = $desiredValues }
        Save-Ledger
    }
}
function Record-Verification([string] $Key, [string] $Message) { $script:Failures.Add("${Key}: $Message") }
function Verify-Registries {
    foreach ($repository in @('asciiwall', 'asciiwall-library')) {
        $labels = @(Read-Pages "repos/$Owner/$repository/labels")
        foreach ($desired in $script:LabelRegistry) {
            $actual = @($labels | Where-Object { $_.name -ceq $desired.name })
            if ($actual.Count -ne 1 -or $actual[0].color -ine $desired.color -or [string] $actual[0].description -cne [string] $desired.description) { Record-Verification $repository "label registry mismatch: $($desired.name)" }
        }
        $milestones = @(Read-Pages "repos/$Owner/$repository/milestones?state=all")
        foreach ($key in $script:MilestoneRegistry.Keys) {
            if (-not $script:UsedMilestones.Contains($key)) { continue }
            $desired = $script:MilestoneRegistry[$key]
            if ($desired.repository -ne $repository) { continue }
            $actual = @($milestones | Where-Object { $_.title -ceq $desired.title })
            if ($actual.Count -ne 1) { Record-Verification $key 'missing or ambiguous milestone'; continue }
            $script:Milestones[$key] = $actual[0]
            if ($script:ManagedMilestones.Contains($key) -and [string] $actual[0].description -cne [string] $desired.description) { Record-Verification $key 'milestone description mismatch' }
        }
    }
    Refresh-Project
    if ($null -ne $script:Ledger.project) {
        if ($script:Ledger.project.id -cne $script:Project.id) { Record-Verification 'project' 'ledger project identity mismatch' }
        foreach ($name in $script:Ledger.project.fields.Keys) {
            $baseline = $script:Ledger.project.fields[$name]
            $actual = $script:Fields[$name]
            if ($null -eq $actual -or $actual.id -cne $baseline.id) { Record-Verification 'project' "field identity changed: $name"; continue }
            if ($baseline.dataType -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
                $options = @(Field-Options $actual)
                foreach ($option in @(Field-Options $baseline)) {
                    if (@($options | Where-Object { $_.id -ceq $option.id -and $_.name -ceq $option.name -and $_.color -ceq $option.color -and [string] $_.description -ceq [string] $option.description }).Count -ne 1) { Record-Verification 'project' "existing option ID or user metadata changed: $name/$($option.id)" }
                }
            }
        }
    }
    if ($script:Project.title -cne $script:ProjectRegistry.desired_title) { Record-Verification 'project' 'title mismatch' }
    foreach ($desired in $script:ProjectRegistry.fields) {
        $field = $script:Fields[$desired.name]
        $kind = $desired.kind -replace '^NATIVE_', ''
        if ($null -eq $field -or $field.dataType -ne $kind) { Record-Verification 'project' "missing or incompatible field $($desired.name)"; continue }
        if ($kind -in @('SINGLE_SELECT', 'MULTI_SELECT')) {
            $options = @(Field-Options $field)
            foreach ($name in $desired.options) { if (@($options | Where-Object { $_.name -ceq $name }).Count -ne 1) { Record-Verification 'project' "missing/ambiguous option $($desired.name)=$name" } }
        }
    }
    $linked = @(); $cursor = $null
    do {
        $data = Graph-Read 'query($id:ID!,$cursor:String){node(id:$id){... on ProjectV2{repositories(first:100,after:$cursor){nodes{nameWithOwner} pageInfo{hasNextPage endCursor}}}}}' @{ id = $script:Project.id; cursor = $cursor }
        $linked += @($data.node.repositories.nodes.nameWithOwner)
        $cursor = $data.node.repositories.pageInfo.endCursor
    } while ($data.node.repositories.pageInfo.hasNextPage)
    foreach ($repository in $script:ProjectRegistry.repositories) { if ($repository -cnotin $linked) { Record-Verification 'project' "repository not linked: $repository" } }
}
function Verify-All {
    $script:Failures.Clear()
    if ($Apply) {
        $script:Ledger.verification = @{ state = 'in-progress'; started_at = [DateTimeOffset]::UtcNow.ToString('o'); scope = @($script:Order | ForEach-Object { Key $_ }) }
        foreach ($ticket in $script:Order) {
            $entry = $script:Ledger.tickets[(Key $ticket)]
            if ($null -ne $entry) { $entry.verification = @{ state = 'in-progress' } }
        }
        Save-Ledger
    }
    Verify-Registries
    Refresh-Inventory
    $script:Identities = @{}
    foreach ($ticket in $script:Order) {
        $identity = Find-Identity $ticket
        if ($null -eq $identity) { Record-Verification (Key $ticket) 'issue missing'; continue }
        # Never use mutation responses or inventory summaries for final metadata verification.
        $script:Identities[(Key $ticket)] = Read-Api "repos/$Owner/$($ticket.repository)/issues/$($identity.number)"
        $detail = $script:Identities[(Key $ticket)]
        if ($detail.id -ne $identity.id -or $detail.node_id -cne $identity.node_id -or $detail.number -ne $identity.number -or $detail.html_url -cne $identity.html_url) { Record-Verification (Key $ticket) 'individual issue refetch identity mismatch' }
    }
    Refresh-Items
    foreach ($ticket in $script:Order) {
        $key = Key $ticket; $remote = $script:Identities[$key]
        if ($null -eq $remote) { continue }
        if ($remote.title -cne $ticket.title) { Record-Verification $key 'title mismatch' }
        if ($ExpectedLogin -and $remote.user.login -cne $ExpectedLogin) { Record-Verification $key "issue author is not $ExpectedLogin" }
        try {
            $body = Render-Body $ticket
            if ([string] $remote.body -cne $body -or (Hash ([string] $remote.body)) -cne (Hash $body)) { Record-Verification $key 'final body/hash mismatch' }
        } catch { Record-Verification $key $_.Exception.Message }
        $managed = @($remote.labels | ForEach-Object name | Where-Object { -not [string]::IsNullOrEmpty($_) -and (Is-ManagedLabel $_) })
        if (-not (Same-Set $managed @(Desired-Labels $ticket))) { Record-Verification $key 'managed label set mismatch' }
        $recordedDesired = $script:Ledger.tickets[$key].desired
        if ($null -ne $recordedDesired -and $recordedDesired.source_sha256 -ceq $ticket._source_hash -and -not (Same-Set @($remote.labels | ForEach-Object name) @($recordedDesired.labels))) { Record-Verification $key 'exact submitted labels changed' }
        $milestone = $script:Milestones[(Milestone-Key $ticket)]
        if ($null -eq $milestone -or $remote.milestone.number -ne $milestone.number -or $remote.milestone.title -cne $script:MilestoneRegistry[(Milestone-Key $ticket)].title) { Record-Verification $key 'native milestone mismatch' }
        if ($remote.type.name -cne (Native-Type $ticket)) { Record-Verification $key 'native Task/Bug/Feature type mismatch or unavailable' }
        $endpoint = "repos/$Owner/$($ticket.repository)/issues/$($remote.number)"
        $parent = Read-Api "$endpoint/parent" -AllowNotFound
        $expectedParent = if ($ticket.parent) { $script:Identities[(Key (Resolve-Ticket $ticket.parent $ticket))] } else { $null }
        if (($null -eq $parent) -ne ($null -eq $expectedParent) -or ($null -ne $parent -and $parent.id -ne $expectedParent.id)) { Record-Verification $key 'native parent mismatch' }
        $blockers = @(Read-Pages "$endpoint/dependencies/blocked_by")
        $expectedBlockers = @($ticket.depends_on | ForEach-Object { $script:Identities[(Key (Resolve-Ticket $_ $ticket))] })
        if (-not (Same-Set @($blockers.id) @($expectedBlockers.id))) { Record-Verification $key 'exact native blocker IDs mismatch' }
        $item = $script:ProjectItems[$remote.node_id]
        if ($null -eq $item) { Record-Verification $key 'actual issue is missing from project'; continue }
        $values = Read-ItemValues $item.id $remote.node_id
        foreach ($field in $script:ProjectRegistry.fields | Where-Object { $_.kind -notlike 'NATIVE_*' }) {
            try {
                $payload = Field-Payload $field $script:Fields[$field.name] $ticket
                if (-not (Field-Matches $payload $values[$field.name])) { Record-Verification $key "project field mismatch: $($field.name) desired $(Json $payload)" }
            } catch { Record-Verification $key $_.Exception.Message }
        }
        if ($values['Repository'].repository.nameWithOwner -cne "$Owner/$($ticket.repository)") { Record-Verification $key 'project native Repository mismatch' }
        if ($values['Milestone'].milestone.number -ne $remote.milestone.number -or $values['Milestone'].milestone.title -cne $remote.milestone.title) { Record-Verification $key 'project native Milestone mismatch' }
        $actualAssignees = if ($values.ContainsKey('Assignees') -and $null -ne $values['Assignees'].users) { @($values['Assignees'].users.nodes.login | Where-Object { $null -ne $_ -and [string] $_ -ne '' }) } else { @() }
        if (-not (Same-Set $actualAssignees @($remote.assignees.login | Where-Object { $null -ne $_ -and [string] $_ -ne '' }))) { Record-Verification $key 'project native Assignees mismatch' }
        if ($Apply) {
            $ticketFailed = @($script:Failures | Where-Object { $_.StartsWith($key + ': ', [StringComparison]::Ordinal) }).Count -gt 0
            $script:Ledger.tickets[$key].verification = @{
                state = $(if ($ticketFailed) { 'failed' } else { 'verified' })
                checked_at = [DateTimeOffset]::UtcNow.ToString('o'); body_sha256 = Hash ([string] $remote.body)
                issue_id = $remote.id; node_id = $remote.node_id; item_id = $item.id
                parent_id = $(if ($parent) { $parent.id } else { $null }); blocker_ids = @($blockers.id)
                milestone_number = $remote.milestone.number; native_type = $remote.type.name
                labels = @($remote.labels | ForEach-Object name | Where-Object { -not [string]::IsNullOrEmpty($_) }); fields = $values; issue_state = $remote.state
            }
            if (-not $ticketFailed) {
                $script:Ledger.tickets[$key].creation_state = 'verified'
                $script:Ledger.tickets[$key].metadata_state = 'verified'
                $script:Ledger.tickets[$key].relationships.state = 'verified'
                $script:Ledger.tickets[$key].project.state = 'verified'
            }
            Save-Ledger
        }
    }
    if ($Apply) {
        foreach ($ticket in $script:Order) {
            $entry = $script:Ledger.tickets[(Key $ticket)]
            if ($entry.verification.state -eq 'in-progress') { $entry.verification.state = 'failed' }
        }
        $script:Ledger.verification = @{ state = $(if ($script:Failures.Count -eq 0) { 'verified' } else { 'failed' }); checked_at = [DateTimeOffset]::UtcNow.ToString('o'); scope = @($script:Order | ForEach-Object { Key $_ }); failures = @($script:Failures.ToArray()) }
        if ($script:Failures.Count -eq 0) {
            foreach ($key in $script:CurrentOperations) { if ($script:Ledger.operations.ContainsKey($key)) { $script:Ledger.operations[$key].state = 'verified' } }
            foreach ($entry in $script:Ledger.registries.labels.Values) { $entry.state = 'verified' }
            foreach ($key in $script:ManagedMilestones) {
                if ($script:Ledger.registries.milestones.ContainsKey($key)) { $script:Ledger.registries.milestones[$key].state = 'verified' }
            }
            $script:Ledger.project.state = 'verified'
            $script:Ledger.workflow.state = 'verified'
            $script:Ledger.workflow.phase = 'complete'
        }
        Save-Ledger
    }
    Assert ($script:Failures.Count -eq 0) ("Exhaustive verification failed:`n" + ($script:Failures -join "`n"))
    Write-Output (Json @{ mode = $(if ($VerifyOnly) { 'verify-only' } else { 'apply' }); state = 'verified'; tickets = $script:Order.Count; milestones = @($script:RequestedMilestones); project = $script:Project.id; ledger_written = [bool] $Apply })
}

try {
    Assert (@(@($Apply, $DryRun, $VerifyOnly) | Where-Object { $_ }).Count -le 1) 'Use only one of -Apply, -DryRun, -VerifyOnly.'
    Assert ($Owner -match '^[A-Za-z0-9][A-Za-z0-9-]*$' -and $ProjectNumber -gt 0) 'Invalid GitHub owner or project number.'
    Assert ([string]::IsNullOrEmpty($SourceRef) -or -not [string]::IsNullOrWhiteSpace($SourceRef)) 'SourceRef cannot be whitespace.'
    Assert ($RetryOperation.Count -eq 0 -or $Apply) '-RetryOperation is only valid with explicit -Apply.'
    Assert-ExternalRoot
    $validation = Run-Process $PythonCommand @((Join-Path $PSScriptRoot 'validate-backlog.py'), '--backlog-root', $script:BacklogRoot, '--source-root', $script:RepoRoot)
    if ($validation.Out) { [Console]::Error.Write($validation.Out) }
    if ($validation.Error) { [Console]::Error.Write($validation.Error) }
    Assert ($validation.ExitCode -eq 0) "Backlog validator failed with exit $($validation.ExitCode); no GitHub or ledger operation attempted."
    $script:SourceSha = 'ffffffffffffffffffffffffffffffffffffffff'
    Load-Source
    if (-not $Apply -and -not $VerifyOnly) {
        $script:SourceRefRequested = if ($SourceRef) { $SourceRef } else { 'main' }
        $script:SourceSha = $script:SourceRefRequested
        Write-Output (Json @{
            mode = 'dry-run'; offline = $true; backlog_root = $script:BacklogRoot; source_root = $script:RepoRoot
            source_ref = $script:SourceRefRequested
            owner = $Owner; project_number = $ProjectNumber; labels_per_repository = $script:LabelRegistry.Count
            milestones = $script:UsedMilestones.Count; requested_milestones = @($script:RequestedMilestones); tickets = $script:Order.Count; source_tickets = $script:Tickets.Count; ledger = $script:LedgerPath
            derived_labels = @($script:DerivedLabels | ForEach-Object { @{ name = $_.name; ticket_ids = [string[]] @($_.ticket_ids); source = $_.source; meaning = $_.meaning } })
            creation_order = @($script:Order | ForEach-Object { Key $_ })
            operations = @($script:Order | ForEach-Object {
                $ticket = $_; $projectValues = @{}
                foreach ($field in $script:ProjectRegistry.fields | Where-Object { $_.kind -notlike 'NATIVE_*' }) { $projectValues[$field.name] = Desired-Field $field $ticket }
                @{ ticket = Key $ticket; kind = $ticket.kind; native_type = Native-Type $ticket; labels = @(Desired-Labels $ticket); milestone = Milestone-Key $ticket; parent = $ticket.parent; blockers = @($ticket.depends_on); source_sha256 = $ticket._source_hash; initial_body_sha256 = Hash (Render-Body $ticket -Initial); project_values = $projectValues }
            })
            phases = @('registry/schema preflight', 'labels/milestones', 'project fields/repositories', 'identify/create', 'final bodies/metadata', 'native parent/blockers', 'actual project items/fields', 'independent exhaustive verification')
        })
    } else {
        if ($ExpectedLogin) {
            $principal = Read-Api 'user'
            Assert ($principal.login -ceq $ExpectedLogin) "Authenticated account is '$($principal.login)', not '$ExpectedLogin'; no mutation attempted."
        }
        if ($Apply) {
            try { $script:Lock = [IO.FileStream]::new((Join-Path $script:BacklogRoot 'publication-ledger.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
            catch { throw 'Another publisher holds the external publication lock; no remote mutation attempted.' }
        }
        Load-Ledger
        if ($Apply) { $script:Ledger.workflow = @{ state = 'in-progress'; phase = 'schema preflight'; started_at = [DateTimeOffset]::UtcNow.ToString('o'); scope = @($script:Order | ForEach-Object { Key $_ }); requested_milestones = @($script:RequestedMilestones); expected_login = $ExpectedLogin } }
        Require-ProjectSchema
        $requestedRef = if ($SourceRef) { $SourceRef } elseif ($script:Ledger.source_ref) { [string] $script:Ledger.source_ref } else { 'main' }
        if ($SourceRef -and $script:Ledger.source_ref -and $SourceRef -cne $script:Ledger.source_ref) {
            throw "SourceRef '$SourceRef' conflicts with ledger-pinned revision '$($script:Ledger.source_ref)' ($($script:Ledger.source_sha)); use the pinned ref or review/update the ledger deliberately."
        }
        $refEndpoint = Escape-Segment $(if (-not $SourceRef -and $script:Ledger.source_sha) { [string] $script:Ledger.source_sha } else { $requestedRef })
        $revision = Read-Api "repos/$Owner/asciiwall/commits/$refEndpoint"
        Assert ([bool] $revision.sha) "SourceRef '$requestedRef' does not resolve in the main documentation repository."
        if ($script:Ledger.source_sha -and $revision.sha -cne $script:Ledger.source_sha) { throw "SourceRef '$requestedRef' now resolves to $($revision.sha), not ledger-pinned $($script:Ledger.source_sha); refusing documentation body drift." }
        $script:SourceRefRequested = $requestedRef
        $script:SourceSha = [string] $revision.sha
        $script:Ledger.source_ref = $requestedRef
        $script:Ledger.source_sha = $script:SourceSha
        if ($Apply) { Save-Ledger }
        if ($VerifyOnly) {
            Verify-All
        } else {
            $script:Ledger.workflow.phase = 'labels/milestones'; Save-Ledger
            Ensure-Registries
            $script:Ledger.workflow.phase = 'project configuration'; Save-Ledger
            Ensure-Project
            $script:Ledger.workflow.phase = 'issue creation/final bodies'; Save-Ledger
            Ensure-Issues
            Ensure-Relationships
            $script:Ledger.workflow.phase = 'project items/fields'; Save-Ledger
            Ensure-Items
            $script:Ledger.workflow.phase = 'independent verification'; Save-Ledger
            Verify-All
        }
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    if ($Apply -and $null -ne $script:Ledger -and $null -ne $script:Lock) {
        try {
            $script:Ledger.last_failure = @{ at = [DateTimeOffset]::UtcNow.ToString('o'); error = $_.Exception.Message }
            if ($null -ne $script:Ledger.workflow) { $script:Ledger.workflow.state = 'failed' }
            if ($script:Ledger.verification.state -eq 'in-progress') { $script:Ledger.verification.state = 'failed' }
            foreach ($ticket in $script:Order) {
                $entry = $script:Ledger.tickets[(Key $ticket)]
                if ($entry.verification.state -eq 'in-progress') { $entry.verification.state = 'failed' }
            }
            Save-Ledger
            $pending = @($script:Ledger.operations.GetEnumerator() | Where-Object { $_.Value.state -in @('intent', 'unknown', 'succeeded') } | ForEach-Object { @{ operation = $_.Key; method = $_.Value.method; endpoint = $_.Value.endpoint; payload = $_.Value.payload; state = $_.Value.state } })
            [Console]::Error.WriteLine('Exact unverified operations: ' + (Json $pending))
        } catch { [Console]::Error.WriteLine('Could not persist failure state: ' + $_.Exception.Message) }
    }
    exit 1
} finally {
    if ($null -ne $script:Lock) { $script:Lock.Dispose() }
}
