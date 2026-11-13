#requires -Version 7.0
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

# -----------------------------------------------------------------------------
# Regression Test Suite: Project View Audit Orientation, Sort Order,
# and Ordered Vector Canonicalization.
#
# Validates consumer-visible axis orientation (BOARD layout verticalGroupByFields
# as primary columns vs groupByFields as horizontal swimlanes; TABLE layout
# groupByFields as primary with no vertical groups) and ordered sort vectors
# via actual Set-ProjectView and Add-Operation AST-loaded from configure-github.ps1.
# Confirms Canonical-OrderedVector normalizes scalar vs singleton vector values
# while preserving cardinality, order, and distinction between null and empty.
# Pure in-memory AST execution; no network operations or external mutations.
# -----------------------------------------------------------------------------

# Initialize script-scope state required by Set-ProjectView and Add-Operation
$script:ProjectId = 'PVT_kwDOEF_JvM4BrI0s'
$script:Operations = [Collections.Generic.List[object]]::new()
$script:Failures = [Collections.Generic.List[string]]::new()
$script:PendingManual = [Collections.Generic.List[string]]::new()
$script:ProjectViewIds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:Changed = $false
$script:DelayMilliseconds = 0
$script:Apply = $false
$Apply = $false

# Locate and parse configure-github.ps1 via AST without executing top-level code or CLI
$configureScript = Join-Path $PSScriptRoot 'configure-github.ps1'
if (-not (Test-Path -LiteralPath $configureScript)) {
    throw "Target configuration script not found: $configureScript"
}

$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($configureScript, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) {
    throw "Syntax errors parsing '$configureScript': $($parseErrors -join '; ')"
}

$targetFunctions = @(
    'Assert',
    'Json',
    'Canonical',
    'Canonical-OrderedVector',
    'Add-Operation',
    'Set-ProjectView'
)

$functionAsts = $ast.FindAll({ $args[0] -is [Management.Automation.Language.FunctionDefinitionAst] }, $true)
$loaded = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

foreach ($fnAst in $functionAsts) {
    if ($fnAst.Name -in $targetFunctions) {
        . ([scriptblock]::Create($fnAst.Extent.Text))
        [void]$loaded.Add($fnAst.Name)
    }
}

foreach ($name in $targetFunctions) {
    if (-not $loaded.Contains($name)) {
        throw "Missing required function '$name' extracted from '$configureScript'."
    }
}

function Reset-TestState {
    $script:Operations.Clear()
    $script:Failures.Clear()
    $script:PendingManual.Clear()
    $script:ProjectViewIds.Clear()
    $script:Changed = $false
}

function Get-AuditOperation([string] $Name) {
    $matched = @($script:Operations | Where-Object { $_.name -eq $Name })
    if ($matched.Count -eq 0) { return $null }
    return $matched[-1]
}

# Isolated project field definitions for test fixtures
$fixtureFields = @(
    [pscustomobject]@{ id = 'f_title'; name = 'Title' }
    [pscustomobject]@{ id = 'f_phase'; name = 'Phase' }
    [pscustomobject]@{ id = 'f_status'; name = 'Status' }
    [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }
    [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }
    [pscustomobject]@{ id = 'f_milestone'; name = 'Milestone' }
)

# -----------------------------------------------------------------------------
# Test Group 1: Canonical-OrderedVector Unit Semantics and Bug Reproduction
# -----------------------------------------------------------------------------

# 1A: 1-key scalar vs singleton collection normalization
$canonicalScalar = Canonical-OrderedVector 'Phase'
$canonicalArray = Canonical-OrderedVector @('Phase')
Assert ($canonicalScalar -ceq $canonicalArray) "Canonical-OrderedVector must normalize scalar 'Phase' to match singleton collection @('Phase')."
Assert ($canonicalScalar -ceq '["Phase"]') "Canonical-OrderedVector must produce singleton JSON array with the field string."

# 1B: Pre-fix Canonical without Canonical-OrderedVector reproduces the array-vs-scalar regression
$rawScalar = Canonical 'Phase'
$rawArray = Canonical @('Phase')
Assert ($rawScalar -ceq '"Phase"') "Pre-fix Canonical of scalar string is a scalar JSON string."
Assert ($rawArray -ceq '["Phase"]') "Pre-fix Canonical of array is a JSON array."
Assert ($rawScalar -cne $rawArray) "Pre-fix Canonical failed on scalar vs singleton collection, reproducing prepatch board comparison failure."

# 1C: 2-key vector ordering is strictly preserved and significant
$twoKeyAsc = Canonical-OrderedVector @('Priority', 'Target Date')
$twoKeyDesc = Canonical-OrderedVector @('Target Date', 'Priority')
Assert ($twoKeyAsc -ceq '["Priority","Target Date"]') "Canonical-OrderedVector must preserve exact element ordering for two-key vectors."
Assert ($twoKeyDesc -ceq '["Target Date","Priority"]') "Canonical-OrderedVector must preserve inverted element ordering."
Assert ($twoKeyAsc -cne $twoKeyDesc) "Swapped two-key vector order must not match under Canonical-OrderedVector."

# 1D: Cardinality difference (1-key vs 2-key vector)
$oneKey = Canonical-OrderedVector @('Priority')
Assert ($oneKey -cne $twoKeyAsc) "Single-key vector must not match two-key vector under Canonical-OrderedVector."

# 1E: Null vs Empty collection distinction
$nullVec = Canonical-OrderedVector $null
$emptyVec = Canonical-OrderedVector @()
Assert ($nullVec -ceq 'null') "Canonical-OrderedVector `$null must be 'null'."
Assert ($emptyVec -ceq '[]') "Canonical-OrderedVector @() must be '[]'."
Assert ($nullVec -cne $emptyVec) "Null and empty collection must remain distinct in Canonical-OrderedVector."

# -----------------------------------------------------------------------------
# Test Group 2: Board View Layout - Two Axes and Two Sort Keys via Set-ProjectView
# -----------------------------------------------------------------------------

$boardViewDefinition = @{
    name = 'Roadmap Board'
    layout = 'BOARD_LAYOUT'
    filter = 'is:open'
    visible_fields = @('Title', 'Phase', 'Status', 'Priority', 'Target Date')
    group_by = 'Phase'               # Primary axis: vertical columns
    secondary_group_by = 'Status'     # Secondary axis: horizontal swimlanes
    sort_by = @('Priority', 'Target Date') # Ordered 2-key sort
    slice_by = $null
}

# 2A: Board matching configuration verifies both axes and 2-sort-order
Reset-TestState
$matchingBoardExisting = [pscustomobject]@{
    id = 'v_board_01'
    name = 'Roadmap Board'
    layout = 'BOARD_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_phase', 'f_status', 'f_priority', 'f_date')
    }
    verticalGroupByFields = @([pscustomobject]@{ id = 'f_phase'; name = 'Phase' })
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' },
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' }
    )
}
$project2A = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($matchingBoardExisting) }
}
Set-ProjectView $project2A $boardViewDefinition

$opView2A = Get-AuditOperation 'view:Roadmap Board'
$opApi2A = Get-AuditOperation 'view-api:Roadmap Board'
$opSlice2A = Get-AuditOperation 'view-slice:Roadmap Board'
Assert ($null -ne $opView2A -and $opView2A.state -ceq 'verified') 'Board base view properties must be verified.'
Assert ($null -ne $opApi2A) 'view-api:Roadmap Board operation must be recorded.'
Assert ($opApi2A.state -ceq 'verified') 'Board view with correct vertical columns, swimlanes, and two sort keys must evaluate to verified.'
Assert ((Canonical-OrderedVector $opApi2A.desired.primary_group_by) -ceq (Canonical-OrderedVector $opApi2A.observed.primary_group_by)) 'Primary vertical group must match.'
Assert ((Canonical-OrderedVector $opApi2A.desired.secondary_group_by) -ceq (Canonical-OrderedVector $opApi2A.observed.secondary_group_by)) 'Secondary swimlane group must match.'
Assert ((Canonical-OrderedVector $opApi2A.desired.sort_by) -ceq (Canonical-OrderedVector $opApi2A.observed.sort_by)) 'Ordered 2-key sort must match.'
Assert ($null -eq $opSlice2A) 'Board view without slice_by must not emit view-slice operation.'

# 2B: Swapped axes (vertical columns Phase and swimlanes Status inverted) remains pending
Reset-TestState
$swappedAxesBoardExisting = [pscustomobject]@{
    id = 'v_board_02'
    name = 'Roadmap Board'
    layout = 'BOARD_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_phase', 'f_status', 'f_priority', 'f_date')
    }
    verticalGroupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' }) # Inverted
    groupByFields = @([pscustomobject]@{ id = 'f_phase'; name = 'Phase' })          # Inverted
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' },
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' }
    )
}
$project2B = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($swappedAxesBoardExisting) }
}
Set-ProjectView $project2B $boardViewDefinition

$opApi2B = Get-AuditOperation 'view-api:Roadmap Board'
Assert ($null -ne $opApi2B -and $opApi2B.state -ceq 'pending') 'Board view with inverted vertical columns and swimlanes must evaluate to pending.'
Assert ($script:PendingManual -contains "View 'Roadmap Board': native API grouping or ordered sort fields differ.") 'Pending manual message recorded for swapped axes.'

# 2C: Swapped two-sort-order remains pending
Reset-TestState
$swappedSortBoardExisting = [pscustomobject]@{
    id = 'v_board_03'
    name = 'Roadmap Board'
    layout = 'BOARD_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_phase', 'f_status', 'f_priority', 'f_date')
    }
    verticalGroupByFields = @([pscustomobject]@{ id = 'f_phase'; name = 'Phase' })
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' }, # Inverted order
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' }
    )
}
$project2C = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($swappedSortBoardExisting) }
}
Set-ProjectView $project2C $boardViewDefinition

$opApi2C = Get-AuditOperation 'view-api:Roadmap Board'
Assert ($null -ne $opApi2C -and $opApi2C.state -ceq 'pending') 'Board view with inverted sort key order must evaluate to pending.'

# 2D: Missing sort key (1 key instead of 2) remains pending
Reset-TestState
$missingSortBoardExisting = [pscustomobject]@{
    id = 'v_board_04'
    name = 'Roadmap Board'
    layout = 'BOARD_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_phase', 'f_status', 'f_priority', 'f_date')
    }
    verticalGroupByFields = @([pscustomobject]@{ id = 'f_phase'; name = 'Phase' })
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' }
    )
}
$project2D = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($missingSortBoardExisting) }
}
Set-ProjectView $project2D $boardViewDefinition

$opApi2D = Get-AuditOperation 'view-api:Roadmap Board'
Assert ($null -ne $opApi2D -and $opApi2D.state -ceq 'pending') 'Board view with missing sort key must evaluate to pending.'

# -----------------------------------------------------------------------------
# Test Group 3: Table View Layout - Primary Group, No Vertical, and Secondary Slice
# -----------------------------------------------------------------------------

$tableViewDefinition = @{
    name = 'Backlog Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    visible_fields = @('Title', 'Status', 'Priority', 'Target Date', 'Milestone')
    group_by = 'Status'              # Primary group (horizontal groupByFields in Table)
    secondary_group_by = $null       # TABLE_LAYOUT has no secondary API group
    sort_by = @('Priority', 'Target Date')
    slice_by = 'Milestone'           # Slice by is UI-only, verifiable only manually
}

# 3A: Table matching primary + no vertical group + slice produces verified API and manual-only slice
Reset-TestState
$matchingTableExisting = [pscustomobject]@{
    id = 'v_table_01'
    name = 'Backlog Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_status', 'f_priority', 'f_date', 'f_milestone')
    }
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    verticalGroupByFields = @()      # No vertical nested groups in TABLE
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' },
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' }
    )
}
$project3A = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($matchingTableExisting) }
}
Set-ProjectView $project3A $tableViewDefinition

$opApi3A = Get-AuditOperation 'view-api:Backlog Table'
$opSlice3A = Get-AuditOperation 'view-slice:Backlog Table'
Assert ($null -ne $opApi3A -and $opApi3A.state -ceq 'verified') 'Table view with matching primary group, no vertical group, and ordered sorts must evaluate to verified.'
Assert ($null -eq $opApi3A.desired.secondary_group_by) 'Table view desired secondary_group_by must be null.'
Assert ($null -eq $opApi3A.observed.secondary_group_by) 'Table view observed secondary_group_by must normalize empty vertical group to null.'
Assert ($null -ne $opSlice3A) 'view-slice operation must be recorded for view with slice_by.'
Assert ($opSlice3A.state -ceq 'manual-only-unverifiable-by-api') 'view-slice operation state must be manual-only-unverifiable-by-api, never green.'
Assert ($null -eq $opSlice3A.observed) 'view-slice observed must remain null as no public API exposes Slice by.'
Assert ($opSlice3A.desired.slice_by -ceq 'Milestone') 'view-slice desired must record the configured slice_by field.'
Assert ($script:PendingManual -contains "View 'Backlog Table': manually verify Slice by 'Milestone' in the authorized GitHub UI after saving and reloading; no public API can verify this setting.") 'Pending manual message recorded for slice_by.'

# 3B: Table view with wrong primary group remains pending
Reset-TestState
$wrongPrimaryTableExisting = [pscustomobject]@{
    id = 'v_table_02'
    name = 'Backlog Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_status', 'f_priority', 'f_date', 'f_milestone')
    }
    groupByFields = @([pscustomobject]@{ id = 'f_phase'; name = 'Phase' }) # Wrong primary
    verticalGroupByFields = @()
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' },
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' }
    )
}
$project3B = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($wrongPrimaryTableExisting) }
}
Set-ProjectView $project3B $tableViewDefinition

$opApi3B = Get-AuditOperation 'view-api:Backlog Table'
Assert ($null -ne $opApi3B -and $opApi3B.state -ceq 'pending') 'Table view with wrong primary group must evaluate to pending.'

# 3C: Table view with unexpected second hidden vertical group remains pending
Reset-TestState
$hiddenVerticalTableExisting = [pscustomobject]@{
    id = 'v_table_03'
    name = 'Backlog Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_status', 'f_priority', 'f_date', 'f_milestone')
    }
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    verticalGroupByFields = @([pscustomobject]@{ id = 'f_phase'; name = 'Phase' }) # Hidden vertical group
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' },
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' }
    )
}
$project3C = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($hiddenVerticalTableExisting) }
}
Set-ProjectView $project3C $tableViewDefinition

$opApi3C = Get-AuditOperation 'view-api:Backlog Table'
Assert ($null -ne $opApi3C -and $opApi3C.state -ceq 'pending') 'Table view with unexpected second vertical group must evaluate to pending.'

# 3D: Table view with missing sort remains pending
Reset-TestState
$missingSortTableExisting = [pscustomobject]@{
    id = 'v_table_04'
    name = 'Backlog Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_status', 'f_priority', 'f_date', 'f_milestone')
    }
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    verticalGroupByFields = @()
    sortByFields = @() # Missing sorts
}
$project3D = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($missingSortTableExisting) }
}
Set-ProjectView $project3D $tableViewDefinition

$opApi3D = Get-AuditOperation 'view-api:Backlog Table'
Assert ($null -ne $opApi3D -and $opApi3D.state -ceq 'pending') 'Table view with missing sorts must evaluate to pending.'

# 3E: Table view with reordered sort remains pending
Reset-TestState
$reorderedSortTableExisting = [pscustomobject]@{
    id = 'v_table_05'
    name = 'Backlog Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_status', 'f_priority', 'f_date', 'f_milestone')
    }
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    verticalGroupByFields = @()
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_date'; name = 'Target Date' }; direction = 'ASC' },
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' }
    )
}
$project3E = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($reorderedSortTableExisting) }
}
Set-ProjectView $project3E $tableViewDefinition

$opApi3E = Get-AuditOperation 'view-api:Backlog Table'
Assert ($null -ne $opApi3E -and $opApi3E.state -ceq 'pending') 'Table view with reordered sorts must evaluate to pending.'

# -----------------------------------------------------------------------------
# Test Group 4: 1-Key Vector Table View and Comparison Seam Verification
# -----------------------------------------------------------------------------

$singleKeyTableDef = @{
    name = 'Epics Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    visible_fields = @('Title', 'Status', 'Priority')
    group_by = 'Status'
    secondary_group_by = $null
    sort_by = @('Priority')          # 1-key sort vector
    slice_by = $null
}

Reset-TestState
$singleKeyTableExisting = [pscustomobject]@{
    id = 'v_table_06'
    name = 'Epics Table'
    layout = 'TABLE_LAYOUT'
    filter = 'is:open'
    configuration = [pscustomobject]@{
        visibleFieldIds = @('f_title', 'f_status', 'f_priority')
    }
    groupByFields = @([pscustomobject]@{ id = 'f_status'; name = 'Status' })
    verticalGroupByFields = @()
    sortByFields = @(
        [pscustomobject]@{ field = [pscustomobject]@{ id = 'f_priority'; name = 'Priority' }; direction = 'ASC' }
    )
}
$project4 = [pscustomobject]@{
    fields = [pscustomobject]@{ nodes = $fixtureFields }
    views = [pscustomobject]@{ nodes = @($singleKeyTableExisting) }
}
Set-ProjectView $project4 $singleKeyTableDef

$opApi4 = Get-AuditOperation 'view-api:Epics Table'
Assert ($null -ne $opApi4 -and $opApi4.state -ceq 'verified') 'Single-key vector Table view must evaluate to verified.'

# Prove comparison difference between pre-fix Canonical and post-fix Canonical-OrderedVector on 1-key payload
$desiredPayload1Key = @{
    primary_group_by = @('Phase')
    secondary_group_by = $null
    sort_by = @('Priority')
}
$observedPayload1KeyScalar = @{
    primary_group_by = 'Phase'       # Scalar string observed in baseline report
    secondary_group_by = $null
    sort_by = 'Priority'             # Scalar string observed in baseline report
}

# Pre-fix comparison passes raw hashtables to Canonical: fails because "Phase" != ["Phase"]
$preFixEqual = (Canonical $desiredPayload1Key) -ceq (Canonical $observedPayload1KeyScalar)
Assert (-not $preFixEqual) 'Pre-fix Canonical comparison must fail for scalar vs singleton collection, catching the bug.'

# Post-fix comparison normalizes ordered vector fields via Canonical-OrderedVector: succeeds
$postFixEqual = (Canonical-OrderedVector $desiredPayload1Key.primary_group_by) -ceq (Canonical-OrderedVector $observedPayload1KeyScalar.primary_group_by) -and
                (Canonical-OrderedVector $desiredPayload1Key.secondary_group_by) -ceq (Canonical-OrderedVector $observedPayload1KeyScalar.secondary_group_by) -and
                (Canonical-OrderedVector $desiredPayload1Key.sort_by) -ceq (Canonical-OrderedVector $observedPayload1KeyScalar.sort_by)
Assert ($postFixEqual) 'Post-fix comparison via Canonical-OrderedVector must equate scalar and singleton collection.'

Write-Output 'PASS: isolated AST project view audit regression passed; board/table axis orientation, 1-key and 2-key vector canonicalization, and UI-only slice audit state validated; no network or GitHub access.'
