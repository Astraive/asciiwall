#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'publish-issues.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Publisher syntax errors prevent the scope regression.' }
$functionNames = @(
    'Assert',
    'Json',
    'Key',
    'Marker',
    'Milestone-Key',
    'Same-Set',
    'Resolve-Ticket',
    'Identity-Reference',
    'Valid-Identity',
    'Same-Identity',
    'Seed-LedgerIdentityCache',
    'Select-CreationBatch',
    'Save-Ledger',
    'Set-Reconciled',
    'Find-Identity'
)
$loaded = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach ($statement in $ast.EndBlock.Statements) {
    if ($statement -is [Management.Automation.Language.FunctionDefinitionAst] -and $statement.Name -in $functionNames) {
        . ([scriptblock]::Create($statement.Extent.Text))
        [void] $loaded.Add($statement.Name)
    }
}
foreach ($name in $functionNames) {
    if (-not $loaded.Contains($name)) { throw "Missing expected publisher function '$name' for scope regression." }
}

function New-FixtureTickets {
    @(
        @{
            id = 'EPIC-OLD'
            repository = 'app'
            kind = 'epic'
            milestone = 'M0'
            parent = $null
            depends_on = @()
            title = 'Closed existing epic'
            _source_hash = 'sha-epic-old'
        },
        @{
            id = 'WIP-OLD'
            repository = 'app'
            kind = 'task'
            milestone = 'M3'
            parent = 'EPIC-OLD'
            depends_on = @()
            title = 'In-progress existing task'
            _source_hash = 'sha-wip-old'
        },
        @{
            id = 'TASK-NEW'
            repository = 'app'
            kind = 'task'
            milestone = 'M1'
            parent = 'EPIC-OLD'
            depends_on = @('WIP-OLD')
            title = 'New dependent task'
            _source_hash = 'sha-task-new'
        },
        @{
            id = 'ITEM-NEW'
            repository = 'lib'
            kind = 'task'
            milestone = 'L0'
            parent = $null
            depends_on = @('app/TASK-NEW', 'app/EPIC-OLD')
            title = 'New library item'
            _source_hash = 'sha-item-new'
        }
    )
}

function Reset-ScopeContext {
    param(
        [object[]] $Tickets,
        [bool] $ApplyMode = $false,
        [bool] $VerifyMode = $false,
        $Ledger = $null,
        [string] $LedgerPath = ''
    )
    $script:Owner = 'ExampleOrg'
    $script:Apply = $ApplyMode
    $script:VerifyOnly = $VerifyMode
    $script:Utf8 = [Text.UTF8Encoding]::new($false)
    $script:LedgerPath = $LedgerPath
    $script:Ledger = $Ledger
    $script:Tickets = @{}
    $script:TicketsById = @{}
    foreach ($ticket in $Tickets) {
        $script:Tickets[(Key $ticket)] = $ticket
        $script:TicketsById[$ticket.id] = $ticket
    }
    $script:SourceOrder = @($Tickets)
    $script:Order = @()
    $script:Identities = @{}
    $script:Inventory = @{ app = @(); lib = @() }
    $script:ProtectedKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:PublicationKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:UsedMilestones = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:ManagedMilestones = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:CurrentOperations = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:RequestedMilestones = [string[]] @()
    $script:MilestoneRegistry = @{
        'app/M0' = @{ id = 'M0'; repository = 'app'; title = 'M0' }
        'app/M3' = @{ id = 'M3'; repository = 'app'; title = 'M3' }
        'app/M1' = @{ id = 'M1'; repository = 'app'; title = 'M1' }
        'lib/L0' = @{ id = 'L0'; repository = 'lib'; title = 'L0' }
    }
    $script:SourceRefRequested = 'main'
    $script:SourceSha = 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeef'
    $script:CreationBatchPreviewBasis = $null
    $script:CreationBatchDraft = $null
}

function Assert-Throws([scriptblock] $Action, [string] $ExpectedPattern, [string] $FailureMessage) {
    $threw = $false
    try {
        & $Action
    } catch {
        $threw = $true
        Assert ($_.Exception.Message -match $ExpectedPattern) "$FailureMessage (unexpected error: $($_.Exception.Message))"
    }
    Assert $threw $FailureMessage
}

$directory = Join-Path ([IO.Path]::GetTempPath()) ('asciiwall-scope-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($directory)
$ledgerFile = Join-Path $directory 'publication-ledger.json'
$lockFile = Join-Path $directory 'publication-ledger.lock'
$script:Lock = $null

try {
    $remoteEpicOld = @{
        id = 1001
        node_id = 'I_1001'
        number = 11
        html_url = 'https://github.com/ExampleOrg/app/issues/11'
        title = 'Closed existing epic'
        state = 'closed'
        body = "<!-- asciiwall-backlog:v1 repository=app ticket=EPIC-OLD -->`nClosed manually"
    }
    $remoteWipOld = @{
        id = 1002
        node_id = 'I_1002'
        number = 12
        html_url = 'https://github.com/ExampleOrg/app/issues/12'
        title = 'In-progress existing task'
        state = 'open'
        body = "<!-- asciiwall-backlog:v1 repository=app ticket=WIP-OLD -->`nIn-progress manual notes"
    }
    $remoteTaskNew = @{
        id = 2001
        node_id = 'I_2001'
        number = 21
        html_url = 'https://github.com/ExampleOrg/app/issues/21'
        title = 'New dependent task'
        state = 'open'
        body = "<!-- asciiwall-backlog:v1 repository=app ticket=TASK-NEW -->`nCreated initial body"
    }
    $remoteItemNew = @{
        id = 2002
        node_id = 'I_2002'
        number = 22
        html_url = 'https://github.com/ExampleOrg/lib/issues/22'
        title = 'New library item'
        state = 'open'
        body = "<!-- asciiwall-backlog:v1 repository=lib ticket=ITEM-NEW -->`nCreated library body"
    }

    # 1. Closed and in-progress existing issues are excluded while parent/dependency refs stay intact.
    $fixtureTickets = New-FixtureTickets
    Reset-ScopeContext -Tickets $fixtureTickets -ApplyMode $true -Ledger @{ tickets = @{}; operations = @{} } -LedgerPath $ledgerFile
    $script:Inventory['app'] = @($remoteEpicOld, $remoteWipOld)
    Assert ((Find-Identity $fixtureTickets[0] -ReadOnly).id -eq 1001) 'Find-Identity -ReadOnly failed to resolve closed parent epic.'
    Assert ((Find-Identity $fixtureTickets[1] -ReadOnly).id -eq 1002) 'Find-Identity -ReadOnly failed to resolve in-progress dependency.'
    Assert ($null -eq (Find-Identity $fixtureTickets[2] -ReadOnly)) 'Missing new task unexpectedly matched an existing issue.'
    Assert ($null -eq (Find-Identity $fixtureTickets[3] -ReadOnly)) 'Missing library item unexpectedly matched an existing issue.'

    $emitted = @(Select-CreationBatch)
    Assert ($emitted.Count -eq 0) 'Select-CreationBatch must not emit pipeline objects.'
    Assert ((@($script:Order | ForEach-Object { Key $_ }) -join ',') -ceq 'app/TASK-NEW,lib/ITEM-NEW') 'Existing closed/in-progress issues were not excluded cleanly from mutable order.'
    Assert ($script:ProtectedKeys.Count -eq 2 -and $script:ProtectedKeys.Contains('app/EPIC-OLD') -and $script:ProtectedKeys.Contains('app/WIP-OLD')) 'ProtectedKeys did not match the existing closed/in-progress issues.'
    Assert (-not $script:ProtectedKeys.Contains('app/TASK-NEW') -and -not $script:ProtectedKeys.Contains('lib/ITEM-NEW')) 'Unpublished issues were wrongly placed in ProtectedKeys.'
    Assert ($script:PublicationKeys.Count -eq 4 -and $script:PublicationKeys.Contains('app/EPIC-OLD') -and $script:PublicationKeys.Contains('app/WIP-OLD')) 'PublicationKeys lost protected target keys needed for cross-issue references.'
    Assert ($script:Identities['app/EPIC-OLD'].state -ceq 'closed' -and $script:Identities['app/EPIC-OLD'].html_url -ceq 'https://github.com/ExampleOrg/app/issues/11') 'Closed parent identity was not retained for new task links.'
    Assert ($script:Identities['app/WIP-OLD'].state -ceq 'open' -and $script:Identities['app/WIP-OLD'].html_url -ceq 'https://github.com/ExampleOrg/app/issues/12') 'In-progress blocker identity was not retained for new task links.'
    Assert ((Key (Resolve-Ticket $script:Order[0].parent $script:Order[0])) -ceq 'app/EPIC-OLD') 'New task lost its parent reference to the excluded closed epic.'
    Assert ((Key (Resolve-Ticket $script:Order[0].depends_on[0] $script:Order[0])) -ceq 'app/WIP-OLD') 'New task lost its blocker reference to the excluded in-progress issue.'
    Assert ((Key (Resolve-Ticket $script:Order[1].depends_on[0] $script:Order[1])) -ceq 'app/TASK-NEW') 'Library task lost its cross-repo dependency on the new task.'
    Assert ((Key (Resolve-Ticket $script:Order[1].depends_on[1] $script:Order[1])) -ceq 'app/EPIC-OLD') 'Library task lost its cross-repo dependency on the excluded closed epic.'
    Assert ($script:ManagedMilestones.Count -eq 2 -and $script:ManagedMilestones.Contains('app/M1') -and $script:ManagedMilestones.Contains('lib/L0')) 'ManagedMilestones must contain only milestones with selected mutable tickets.'
    Assert (-not $script:ManagedMilestones.Contains('app/M0') -and -not $script:ManagedMilestones.Contains('app/M3')) 'Protected-only milestones must not be managed.'
    Assert ($script:UsedMilestones.Count -eq 4) 'UsedMilestones must retain all source ticket milestones.'

    # Offline dry-run with null ledger must not require GitHub or existing ledger file.
    Reset-ScopeContext -Tickets (New-FixtureTickets) -ApplyMode $false -VerifyMode $false -Ledger $null -LedgerPath $ledgerFile
    $dryEmitted = @(Select-CreationBatch)
    Assert ($dryEmitted.Count -eq 0) 'Offline Select-CreationBatch emitted pipeline objects.'
    Assert ($script:Order.Count -eq 4 -and $script:ProtectedKeys.Count -eq 0) 'Empty offline preview should select all source tickets as mutable.'
    Assert (-not [IO.File]::Exists($ledgerFile)) 'Offline Select-CreationBatch created a ledger file.'

    # 2. Pinned resume keeps created-but-incomplete issues writable across Apply resume and VerifyOnly.
    $fixtureTickets = New-FixtureTickets
    $pinnedBatch = @{
        schema_version = 1
        state = 'pinned'
        pinned_at = '2026-11-12T00:00:00Z'
        source_ref = 'main'
        source_sha = 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeef'
        source_order = @('app/EPIC-OLD', 'app/WIP-OLD', 'app/TASK-NEW', 'lib/ITEM-NEW')
        mutable_keys = @('app/TASK-NEW', 'lib/ITEM-NEW')
        protected_keys = @('app/EPIC-OLD', 'app/WIP-OLD')
        protected_identities = @{
            'app/EPIC-OLD' = @{ id = 1001; node_id = 'I_1001'; number = 11; url = 'https://github.com/ExampleOrg/app/issues/11' }
            'app/WIP-OLD' = @{ id = 1002; node_id = 'I_1002'; number = 12; url = 'https://github.com/ExampleOrg/app/issues/12' }
        }
        source_hashes = @{
            'app/EPIC-OLD' = 'sha-epic-old'
            'app/WIP-OLD' = 'sha-wip-old'
            'app/TASK-NEW' = 'sha-task-new'
            'lib/ITEM-NEW' = 'sha-item-new'
        }
    }
    $resumeLedger = @{
        schema_version = 1
        creation_batch = $pinnedBatch
        tickets = @{
            'app/EPIC-OLD' = @{ id = 1001; node_id = 'I_1001'; number = 11; url = 'https://github.com/ExampleOrg/app/issues/11'; creation_state = 'verified'; verification = @{ state = 'verified' } }
            'app/WIP-OLD' = @{ id = 1002; node_id = 'I_1002'; number = 12; url = 'https://github.com/ExampleOrg/app/issues/12'; creation_state = 'verified'; verification = @{ state = 'verified' } }
            'app/TASK-NEW' = @{ id = 2001; node_id = 'I_2001'; number = 21; url = 'https://github.com/ExampleOrg/app/issues/21'; creation_state = 'identified' }
        }
        operations = @{}
    }
    Reset-ScopeContext -Tickets $fixtureTickets -ApplyMode $true -VerifyMode $false -Ledger $resumeLedger -LedgerPath $ledgerFile
    $script:Identities['app/TASK-NEW'] = $remoteTaskNew
    $resumeEmitted = @(Select-CreationBatch)
    Assert ($resumeEmitted.Count -eq 0) 'Pinned Select-CreationBatch emitted pipeline objects.'
    Assert ((@($script:Order | ForEach-Object { Key $_ }) -join ',') -ceq 'app/TASK-NEW,lib/ITEM-NEW') 'Created-but-incomplete issue app/TASK-NEW was excluded on pinned Apply resume.'
    Assert (-not $script:ProtectedKeys.Contains('app/TASK-NEW')) 'Created-but-incomplete issue app/TASK-NEW was reclassified as protected.'
    Assert ($script:ProtectedKeys.Count -eq 2 -and $script:Identities['app/EPIC-OLD'].id -eq 1001 -and $script:Identities['app/WIP-OLD'].id -eq 1002) 'Pinned resume failed to preserve/seed protected identities.'
    Assert ($script:Identities['app/TASK-NEW'].id -eq 2001) 'Pinned resume dropped cached identity of created-but-incomplete ticket.'

    Reset-ScopeContext -Tickets $fixtureTickets -ApplyMode $false -VerifyMode $true -Ledger $resumeLedger -LedgerPath $ledgerFile
    $script:Identities['app/TASK-NEW'] = $remoteTaskNew
    $script:Identities['lib/ITEM-NEW'] = $remoteItemNew
    Select-CreationBatch
    Assert ((@($script:Order | ForEach-Object { Key $_ }) -join ',') -ceq 'app/TASK-NEW,lib/ITEM-NEW') 'VerifyOnly pinned selection dropped newly created issues from mutable verification scope.'
    Assert ($script:ProtectedKeys.Count -eq 2) 'VerifyOnly pinned selection altered protected keys.'

    # 3. Read-only ledger preservation under Find-Identity -ReadOnly and Select-CreationBatch (even with Apply + lock held).
    $initialLedger = @{
        schema_version = 1
        owner = 'ExampleOrg'
        project_number = 4
        tickets = @{
            'app/EPIC-OLD' = @{
                id = 1001
                node_id = 'I_1001'
                number = 11
                url = 'https://github.com/ExampleOrg/app/issues/11'
                marker = '<!-- asciiwall-backlog:v1 repository=app ticket=EPIC-OLD -->'
                source_sha256 = 'prior-pinned-sha'
                creation_state = 'verified'
                metadata_state = 'verified'
                desired = @{ body_sha256 = 'prior-body-sha' }
                verification = @{ state = 'verified'; issue_state = 'closed' }
            }
            'app/WIP-OLD' = @{
                id = 1002
                node_id = 'I_1002'
                number = 12
                url = 'https://github.com/ExampleOrg/app/issues/12'
                marker = '<!-- asciiwall-backlog:v1 repository=app ticket=WIP-OLD -->'
                source_sha256 = 'prior-wip-sha'
                creation_state = 'verified'
                verification = @{ state = 'verified'; issue_state = 'open' }
            }
        }
        operations = @{
            'create:app/EPIC-OLD' = @{
                state = 'verified'
                payload = @{ title = 'Closed existing epic' }
            }
        }
    }
    [IO.File]::WriteAllText($ledgerFile, ((ConvertTo-Json -InputObject $initialLedger -Depth 100 -Compress) + "`n"), [Text.UTF8Encoding]::new($false))
    $beforeDisk = [IO.File]::ReadAllText($ledgerFile)
    $fixtureTickets = New-FixtureTickets
    $fixtureTickets[0]._source_hash = 'changed-local-sha'
    $loadedLedger = $beforeDisk | ConvertFrom-Json -AsHashtable
    $script:Lock = [IO.FileStream]::new($lockFile, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    Reset-ScopeContext -Tickets $fixtureTickets -ApplyMode $true -VerifyMode $false -Ledger $loadedLedger -LedgerPath $ledgerFile
    $script:Inventory['app'] = @($remoteEpicOld, $remoteWipOld)

    $roEpic = Find-Identity $fixtureTickets[0] -ReadOnly
    $roWip = Find-Identity $fixtureTickets[1] -ReadOnly
    Select-CreationBatch
    Assert ($roEpic.id -eq 1001 -and $roWip.id -eq 1002) 'Find-Identity -ReadOnly failed to return existing identities.'
    Assert ($script:Ledger.tickets['app/EPIC-OLD'].creation_state -ceq 'verified') 'Find-Identity -ReadOnly overwrote protected creation_state.'
    Assert ($script:Ledger.tickets['app/EPIC-OLD'].source_sha256 -ceq 'prior-pinned-sha') 'Find-Identity -ReadOnly overwrote protected source_sha256.'
    Assert ($script:Ledger.tickets['app/EPIC-OLD'].desired.body_sha256 -ceq 'prior-body-sha') 'Find-Identity -ReadOnly altered protected desired digest.'
    Assert ($script:Ledger.tickets['app/EPIC-OLD'].verification.state -ceq 'verified') 'Find-Identity -ReadOnly altered protected verification state.'
    Assert ($script:Ledger.operations['create:app/EPIC-OLD'].state -ceq 'verified') 'Find-Identity -ReadOnly mutated prior create operation state.'
    Assert (-not $script:Ledger.ContainsKey('creation_batch')) 'Select-CreationBatch must not mutate Ledger.creation_batch.'
    Assert (([IO.File]::ReadAllText($ledgerFile)) -ceq $beforeDisk) 'Find-Identity -ReadOnly or Select-CreationBatch wrote to the ledger file on disk.'

    # A subsequent non-readonly Find-Identity for a new writable ticket persists only the new record and preserves protected ledger records.
    $script:Inventory['app'] += $remoteTaskNew
    $rwNew = Find-Identity $fixtureTickets[2]
    Assert ($rwNew.id -eq 2001) 'Non-readonly Find-Identity failed to resolve writable new issue.'
    $afterSave = [IO.File]::ReadAllText($ledgerFile) | ConvertFrom-Json -AsHashtable
    Assert ($afterSave.tickets['app/TASK-NEW'].creation_state -ceq 'identified') 'Writable ticket identity was not saved to ledger.'
    Assert ($afterSave.tickets['app/EPIC-OLD'].creation_state -ceq 'verified' -and $afterSave.tickets['app/EPIC-OLD'].source_sha256 -ceq 'prior-pinned-sha' -and $afterSave.operations['create:app/EPIC-OLD'].state -ceq 'verified') 'Saving a new mutable ticket corrupted protected ledger records.'

    # 4. Unresolved required identities and invalid milestone/scope combinations fail closed without touching the ledger.
    $beforeFailures = [IO.File]::ReadAllText($ledgerFile)

    # 4A: Pinned batch has incomplete protected parent identity needed by new task.
    $badBatch = @{
        schema_version = 1
        state = 'pinned'
        pinned_at = '2026-11-12T00:00:00Z'
        source_ref = 'main'
        source_sha = 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeef'
        source_order = @('app/EPIC-OLD', 'app/WIP-OLD', 'app/TASK-NEW', 'lib/ITEM-NEW')
        mutable_keys = @('app/TASK-NEW', 'lib/ITEM-NEW')
        protected_keys = @('app/EPIC-OLD', 'app/WIP-OLD')
        protected_identities = @{
            'app/EPIC-OLD' = @{ id = 1001; node_id = ''; number = 0; url = '' }
            'app/WIP-OLD' = @{ id = 1002; node_id = 'I_1002'; number = 12; url = 'https://github.com/ExampleOrg/app/issues/12' }
        }
        source_hashes = @{
            'app/EPIC-OLD' = 'sha-epic-old'
            'app/WIP-OLD' = 'sha-wip-old'
            'app/TASK-NEW' = 'sha-task-new'
            'lib/ITEM-NEW' = 'sha-item-new'
        }
    }
    Reset-ScopeContext -Tickets (New-FixtureTickets) -ApplyMode $true -Ledger @{ creation_batch = $badBatch; tickets = @{}; operations = @{} } -LedgerPath $ledgerFile
    Assert-Throws { Select-CreationBatch } 'Pinned protected identity is incomplete' 'Incomplete pinned protected parent identity must fail closed.'

    # 4B: Ledger identity record for required protected parent is incomplete before pinning.
    $incompleteLedger = @{
        tickets = @{ 'app/EPIC-OLD' = @{ id = 1001 } }
        operations = @{}
    }
    Reset-ScopeContext -Tickets (New-FixtureTickets) -ApplyMode $true -Ledger $incompleteLedger -LedgerPath $ledgerFile
    Assert-Throws { Select-CreationBatch } 'Incomplete ledger identity' 'Incomplete ledger identity for protected parent must fail closed.'

    # 4C: Selected mutable ticket depends on an unknown unresolved ticket reference.
    $brokenTickets = New-FixtureTickets
    $brokenTickets[2].depends_on = @('MISSING-DEP')
    Reset-ScopeContext -Tickets $brokenTickets -ApplyMode $true -Ledger @{ tickets = @{}; operations = @{} } -LedgerPath $ledgerFile
    $script:Identities['app/EPIC-OLD'] = $remoteEpicOld
    $script:Identities['app/WIP-OLD'] = $remoteWipOld
    Assert-Throws { Select-CreationBatch } 'Unknown or ambiguous ticket reference' 'Unresolved required dependency reference must fail closed.'

    # 4D: Ledger-recorded protected parent missing from remote inventory fails read-only discovery.
    $missingInventoryLedger = @{
        tickets = @{ 'app/EPIC-OLD' = @{ id = 1001; node_id = 'I_1001'; number = 11; url = 'https://github.com/ExampleOrg/app/issues/11' } }
        operations = @{}
    }
    $fixtureTickets = New-FixtureTickets
    Reset-ScopeContext -Tickets $fixtureTickets -ApplyMode $true -Ledger $missingInventoryLedger -LedgerPath $ledgerFile
    Assert-Throws { [void](Find-Identity $fixtureTickets[0] -ReadOnly) } 'missing from exhaustive open/closed inventory' 'Missing ledger-recorded parent in remote inventory must fail closed.'

    # 4E: -CreateMissing cannot be combined with -Milestone.
    Reset-ScopeContext -Tickets (New-FixtureTickets) -ApplyMode $true -Ledger @{ tickets = @{}; operations = @{} } -LedgerPath $ledgerFile
    $script:RequestedMilestones = [string[]] @('app/M1')
    Assert-Throws { Select-CreationBatch } 'cannot be combined with -Milestone' 'Combining -CreateMissing with -Milestone must fail closed.'

    Assert (([IO.File]::ReadAllText($ledgerFile)) -ceq $beforeFailures) 'Failure paths unexpectedly wrote to the ledger file.'
    Write-Output 'PASS: isolated AST scope selection, pinned resume, read-only ledger preservation, and unresolved identity guards succeeded; no GitHub access.'
} finally {
    if ($null -ne $script:Lock) { $script:Lock.Dispose() }
    if ([IO.Directory]::Exists($directory)) { [IO.Directory]::Delete($directory, $true) }
}
