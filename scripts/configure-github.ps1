#requires -Version 7.0
[CmdletBinding()]
param(
    [switch] $Apply,
    [switch] $VerifyOnly,
    [switch] $ConfigureBranchProtection,
    [string[]] $RequiredCheck = @(),
    [string] $GhCommand = 'gh',
    [string] $Owner = 'Astraive',
    [string] $Repository = 'asciiwall',
    [string] $LibraryRepository = 'asciiwall-library',
    [string] $ReportPath = (Join-Path $PSScriptRoot '../../.issues/reports/github-config-audit.json'),
    [ValidateRange(1, 3600)][int] $CommandTimeoutSeconds = 180
)

$ErrorActionPreference = 'Stop'
$script:Root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$script:Definitions = Join-Path $script:Root '.github/project-management'
$script:RepoSlug = "$Owner/$Repository"
 $script:LibrarySlug = "$Owner/$LibraryRepository"
$script:DelayMilliseconds = 1050
$script:Operations = [Collections.Generic.List[object]]::new()
$script:Failures = [Collections.Generic.List[string]]::new()
$script:PendingManual = [Collections.Generic.List[string]]::new()
$script:RepositoryNodeIds=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
$script:Changed = $false

function Read-Json([string] $Path) { Get-Content -LiteralPath $Path -Raw -Encoding utf8 | ConvertFrom-Json -AsHashtable }
function Json($Value) { ConvertTo-Json -InputObject $Value -Depth 100 -Compress }
function Assert([bool] $Condition, [string] $Message) { if (-not $Condition) { throw $Message } }
function Assert-ExternalReportPath([string] $Path) {
    $full=[IO.Path]::GetFullPath($Path)
    foreach($root in @($script:Root,[IO.Path]::GetFullPath((Join-Path $script:Root '../asciiwall-library')))){
        $prefix=$root.TrimEnd('/','\')+[IO.Path]::DirectorySeparatorChar
        Assert (-not ($full.Equals($root,[StringComparison]::OrdinalIgnoreCase) -or $full.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase))) "Audit report must remain outside both Git repositories: $full"
    }
    $check=if(Test-Path -LiteralPath $full){[IO.FileInfo]::new($full)}else{[IO.DirectoryInfo]::new([IO.Path]::GetDirectoryName($full))}
    while($null -ne $check){
        if(Test-Path -LiteralPath $check.FullName){$item=Get-Item -LiteralPath $check.FullName -Force;Assert (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) "Audit report path cannot traverse symlinks/reparse points: $($check.FullName)"}
        $check = if ($check -is [IO.FileInfo]) { $check.Directory } else { $check.Parent }
    }
    return $full
}
function Validate-Definitions($Labels,$Milestones,$Project) {
    Assert ($Labels.Count -eq 78 -and $Milestones.Count -eq 16 -and $Project.fields.Count -eq 20 -and $Project.views.Count -eq 18) 'Static registry inventory is incomplete.'
    Assert ($Project.project.id -match '^PVT_[A-Za-z0-9_]+$' -and $Project.project.number -eq 4) 'Project registry identity is invalid.'
    Assert (@($Project.project.repositories|Sort-Object -Unique).Count -eq 2 -and $script:RepoSlug -in $Project.project.repositories -and $script:LibrarySlug -in $Project.project.repositories) 'Project registry repository scope does not match the configured repository names.'
    Assert ((@($Labels.name|Sort-Object -Unique).Count) -eq 78) 'Label registry contains duplicate names.'
    foreach($label in $Labels){Assert ($label.name -match '^[a-z0-9][a-z0-9:_-]*$' -and $label.color -match '^[0-9A-Fa-f]{6}$' -and $label.description.Length -le 100) "Invalid label definition '$($label.name)'."}
    Assert ((@($Milestones.id|Sort-Object -Unique).Count) -eq 16) 'Milestone registry contains duplicate stable IDs.'
    foreach($milestone in $Milestones){Assert ($milestone.repository -in @($Repository,$LibraryRepository)) "Milestone '$($milestone.id)' has an unexpected repository.";Assert ($milestone.gate -match '^[A-Z][A-Z0-9-]+$' -and $milestone.goal -and $milestone.entry_conditions.Count -gt 0 -and $milestone.exit_conditions.Count -gt 0) "Milestone '$($milestone.id)' is incomplete."}
    Assert ((@($Project.fields.name|Sort-Object -Unique).Count) -eq 20) 'Project field registry contains duplicate names.'
    Assert ((@($Project.views.name|Sort-Object -Unique).Count) -eq 18) 'Project view registry contains duplicate names.'
    $fieldNames=@($Project.fields.name)
    foreach($field in $Project.fields){Assert ($field.kind -in @('SINGLE_SELECT','MULTI_SELECT','TEXT','NUMBER','NATIVE_REPOSITORY','NATIVE_MILESTONE','NATIVE_ASSIGNEES','NATIVE_TITLE','NATIVE_LABELS','NATIVE_PARENT_ISSUE','NATIVE_SUB_ISSUES_PROGRESS','NATIVE_UPDATED')) "Invalid field kind '$($field.kind)'.";Assert ((@($field.options|Sort-Object -Unique).Count) -eq @($field.options).Count) "Field '$($field.name)' has duplicate options."}
    foreach($view in $Project.views){
        Assert ($view.layout -in @('TABLE_LAYOUT','BOARD_LAYOUT')) "Unsupported view layout '$($view.layout)'."
        Assert (@($view.visible_fields|Where-Object{$_ -notin $fieldNames}).Count -eq 0) "View '$($view.name)' names a visible field absent from the field registry."
        Assert (@($view.Keys|Where-Object{$_ -in @('group_by','secondary_group_by','slice_by','sort_by')}).Count -eq 4) "View '$($view.name)' must use exactly the current grouping and sorting keys."
        Assert (@($view.Keys|Where-Object{($_ -match '(?i)(group|slice|sort)') -and $_ -notin @('group_by','secondary_group_by','slice_by','sort_by')}).Count -eq 0) "View '$($view.name)' contains an unsupported legacy grouping or sorting alias."
        Assert ([string]$view.group_by -in $fieldNames) "View '$($view.name)' names a primary grouping field absent from the field registry."
        Assert (@($view.sort_by).Count -eq 2 -and @($view.sort_by|Sort-Object -Unique).Count -eq 2) "View '$($view.name)' must name two distinct ordered sort fields."
        Assert (@($view.sort_by|Where-Object{$_ -notin $fieldNames}).Count -eq 0) "View '$($view.name)' names a sort field absent from the field registry."
        if($view.layout -eq 'TABLE_LAYOUT'){
            Assert ($null -eq $view.secondary_group_by -and [string]$view.slice_by -in $fieldNames) "TABLE view '$($view.name)' must use one primary group, null secondary_group_by, and a registered slice_by field."
        }else{
            Assert ([string]$view.secondary_group_by -in $fieldNames -and $null -eq $view.slice_by) "BOARD view '$($view.name)' must use primary and secondary grouping and null slice_by."
        }
    }
    Assert ([bool]($Project.fields|Where-Object{$_.name -eq 'Platform' -and $_.kind -eq 'MULTI_SELECT'})) 'Platform must remain a native multi-select definition.'
}
function Invoke-Gh([string[]] $Arguments, [string] $InputText = '') {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $GhCommand; $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false); $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false); $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $start.WorkingDirectory = $script:Root
    foreach ($argument in $Arguments) { [void] $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new(); $process.StartInfo = $start
    if (-not $process.Start()) { throw "Could not start GitHub CLI '$GhCommand'." }
    $stdoutTask = $process.StandardOutput.ReadToEndAsync(); $stderrTask = $process.StandardError.ReadToEndAsync()
    if ($InputText) { $process.StandardInput.Write($InputText) }
    $process.StandardInput.Close()
    if (-not $process.WaitForExit($CommandTimeoutSeconds * 1000)) { try { $process.Kill($true) } catch {}; throw "GitHub CLI command timed out after $CommandTimeoutSeconds seconds." }
    $stdout = $stdoutTask.GetAwaiter().GetResult(); $stderr = $stderrTask.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) { throw "GitHub CLI failed ($($process.ExitCode)): $($stderr.Trim())" }
    return $stdout
}
function Canonical($Value) {
    if ($null -eq $Value) { return 'null' }
    if ($Value -is [Collections.IDictionary]) {
        $parts = foreach ($key in @($Value.Keys | Sort-Object -CaseSensitive)) { "$(Json ([string]$key)):$([string](Canonical $Value[$key]))" }
        return '{' + ($parts -join ',') + '}'
    }
    if ($Value -is [Collections.IEnumerable] -and $Value -isnot [string]) {
        $parts = foreach ($item in $Value) { [string](Canonical $item) }
        return '[' + ($parts -join ',') + ']'
    }
    return Json $Value
}
function Canonical-OrderedVector($Value) {
    if ($null -eq $Value) { return Canonical $null }
    if ($Value -is [Collections.IEnumerable] -and $Value -isnot [string]) { return Canonical $Value }
    return '[' + (Canonical $Value) + ']'
}
function Assert-SingleMutation([string] $Query) {
    # Every supported mutation passes values through variables, never string literals.
    Assert ($Query -notmatch '["#]' -and $Query -match '^\s*mutation\s*\([^{}=]*\)\s*\{') 'Mutations must use the supported variable-only operation shape.'
    $depth = 0
    $parentheses = 0
    $selectionStarted = $false
    $selectionClosed = $false
    for ($index = $Query.IndexOf('{'); $index -lt $Query.Length; $index++) {
        $character = $Query[$index]
        Assert (-not $selectionClosed -or [char]::IsWhiteSpace($character) -or $character -eq '}') 'Exactly one root mutation selection is allowed.'
        switch ($character) {
            '(' { $parentheses++ }
            ')' { $parentheses-- }
            '{' {
                if ($depth -eq 1 -and $parentheses -eq 0) { $selectionStarted = $true }
                $depth++
            }
            '}' {
                $depth--
                if ($depth -eq 1 -and $selectionStarted -and $parentheses -eq 0) { $selectionClosed = $true }
                if ($depth -eq 0) { Assert ($index -eq $Query.TrimEnd().Length - 1) 'Multiple GraphQL operations are forbidden.' }
            }
        }
    }
    Assert ($selectionClosed -and $depth -eq 0 -and $parentheses -eq 0) 'Unbalanced or incomplete mutation selection.'
}
function Assert-AllowedRequest([string] $Method, [string] $Path, $Body) {
    $repos = @($script:RepoSlug, $script:LibrarySlug) | ForEach-Object { [regex]::Escape($_) }
    $repoPattern = '(?:' + ($repos -join '|') + ')'
    if ($Path -eq 'graphql') {
        Assert ($Method -eq 'POST' -and $Body -is [Collections.IDictionary] -and $Body.query) 'Only structured GraphQL requests are allowed.'
        $query = [string]$Body.query
        if ($query -match '^\s*mutation') {
            Assert-SingleMutation $query
            $mutationName=([regex]::Match($query,'^\s*mutation\s*\([^{}=]*\)\s*\{\s*(createProjectV2Field|updateProjectV2Field|createProjectV2View|updateProjectV2View|linkProjectV2ToRepository|updateProjectV2)\b')).Groups[1].Value
            Assert ($mutationName -in @('createProjectV2Field','updateProjectV2Field','createProjectV2View','updateProjectV2View','linkProjectV2ToRepository','updateProjectV2')) 'GraphQL mutation is outside the project-infrastructure allowlist.'
            Assert ([regex]::Matches($query,'\{\s*(?:createProjectV2Field|updateProjectV2Field|createProjectV2View|updateProjectV2View|linkProjectV2ToRepository|updateProjectV2)\b').Count -eq 1) 'Exactly one allowlisted GraphQL mutation is permitted per request.'
            Assert ($query -notmatch '(?i)(?:Issue|Draft|ProjectV2Item)') 'Issue or project-item GraphQL operations are forbidden.'
            $target=$Body.variables.project
            if($Body.variables.projectId){$target=$Body.variables.projectId}
            if($Body.variables.id -and $mutationName -eq 'updateProjectV2'){$target=$Body.variables.id}
            if($mutationName -in @('createProjectV2Field','createProjectV2View')){Assert ($Body.variables.project -eq $script:ProjectId) 'Project creation mutation targets an unapproved project.'}
            if($mutationName -eq 'createProjectV2Field'){Assert ([string]$Body.variables.name -in $script:ProjectFieldNames) 'Project field creation name is not in the static registry.'}
            if($mutationName -eq 'createProjectV2View'){Assert ([string]$Body.variables.name -in $script:ProjectViewNames) 'Project view creation name is not in the static registry.'}
            if($mutationName -eq 'updateProjectV2'){Assert ([string]$Body.variables.title -ceq $script:DesiredProjectTitle) 'Project title update must match the static registry.'}
            if($mutationName -in @('updateProjectV2Field','updateProjectV2View')){$targetId=if($mutationName -eq 'updateProjectV2Field'){[string]$Body.variables.field}else{[string]$Body.variables.id};$allowedIds=if($mutationName -eq 'updateProjectV2Field'){$script:AllowedProjectFieldIds}else{$script:AllowedProjectViewIds};Assert ($null -ne $allowedIds -and $allowedIds.Contains($targetId)) 'GraphQL update targets an unknown infrastructure field/view id.'}
            else{Assert ($target -eq $script:ProjectId) 'GraphQL mutation targets an unapproved project.'}
            if($mutationName -eq 'updateProjectV2Field'){Assert ($query -notmatch '\bname\s*:') 'Project field updates are restricted to select-option values.'}
            if($mutationName -eq 'updateProjectV2'){Assert ($query -match 'title:\$title' -and $query -notmatch '(?i)\b(?:public|visibility|closed|owner)\b') 'Project updates are restricted to renaming.'}
            if($mutationName -eq 'linkProjectV2ToRepository'){Assert ($script:RepositoryNodeIds.Contains([string]$Body.variables.repo) -and $Body.variables.project -eq $script:ProjectId) 'Project link target is outside the repository/project allowlist.'}
            return
        }
        Assert ($Method -eq 'POST' -and $query -match '^\s*query' -and $query -match '\bProjectV2\b|\bRepository\b') 'GraphQL read is outside the approved project/repository introspection scope.'
        Assert ($query -notmatch '(?i)\b(?:issue|draft|ProjectV2Item)\b') 'Issue and project-item GraphQL reads are outside this tool scope.'
        return
    }
    $allowed = $false
    if ($Method -eq 'GET') {
        $allowed = $Path -match "^repos/$repoPattern$" -or
            $Path -match "^repos/$repoPattern/topics$" -or
            $Path -match "^repos/$repoPattern/labels(?:/[^/?]+)?$" -or
            $Path -match "^repos/$repoPattern/milestones(?:\?state=(?:open|closed)&per_page=100(?:&page=\d+)?)?$" -or
            $Path -match "^repos/$repoPattern/(?:secret-scanning|secret-scanning/push-protection|vulnerability-alerts|automated-security-fixes|private-vulnerability-reporting)$" -or
            $Path -match "^repos/$repoPattern/actions/permissions/workflow$" -or
            $Path -match "^repos/$repoPattern/branches/main/protection$"
    } elseif ($Method -eq 'PATCH') {
        $allowed = $Path -match "^repos/$repoPattern$" -or
            $Path -match "^repos/$repoPattern/labels/[^/?]+$" -or
            $Path -match "^repos/$repoPattern/milestones/\d+$"
    } elseif ($Method -eq 'POST') {
        $allowed = $Path -match "^repos/$repoPattern/labels$" -or
            $Path -match "^repos/$repoPattern/milestones$"
    } elseif ($Method -eq 'PUT') {
        $allowed = $Path -match "^repos/$repoPattern/topics$" -or
            $Path -match "^repos/$repoPattern/actions/permissions/workflow$" -or
            $Path -match "^repos/$repoPattern/(?:vulnerability-alerts|automated-security-fixes|private-vulnerability-reporting)$" -or
            $Path -match "^repos/$repoPattern/branches/main/protection$"
    }
    if($allowed -and $Method -in @('PATCH','POST','PUT')){
        Assert ($Body -is [Collections.IDictionary]) 'Infrastructure mutations require a JSON object body.'
        $repoSettings=@('description','has_discussions','allow_squash_merge','allow_merge_commit','allow_rebase_merge','delete_branch_on_merge','web_commit_signoff_required','security_and_analysis')
        if($Method -eq 'PATCH' -and $Path -match "^repos/$repoPattern$"){
            Assert (@($Body.Keys|Where-Object{$_ -notin $repoSettings}).Count -eq 0) 'Repository update body contains non-allowlisted keys.'
            if($Body.security_and_analysis){Assert (@($Body.security_and_analysis.Keys|Where-Object{$_ -notin @('secret_scanning','secret_scanning_push_protection')}).Count -eq 0) 'Only secret-scanning infrastructure settings are allowed.';foreach($entry in $Body.security_and_analysis.Values){Assert ($entry.status -eq 'enabled') 'Security settings may only be enabled.'}}
        } elseif($Path -match '/(?:labels(?:/[^/?]+)?)$'){
            Assert (@($Body.Keys|Where-Object{$_ -notin @('name','color','description')}).Count -eq 0) 'Label mutation body contains non-allowlisted keys.'
        } elseif($Path -match '/milestones(?:/\d+)?$'){
            Assert (@($Body.Keys|Where-Object{$_ -notin @('title','description')}).Count -eq 0) 'Milestone mutation body contains non-allowlisted keys.'
        } elseif($Path -match '/topics$'){
            Assert (@($Body.Keys|Where-Object{$_ -ne 'names'}).Count -eq 0) 'Topic mutation body is invalid.'
        } elseif($Path -match '/actions/permissions/workflow$'){
            Assert (@($Body.Keys|Where-Object{$_ -notin @('default_workflow_permissions','can_approve_pull_request_reviews')}).Count -eq 0 -and $Body.default_workflow_permissions -eq 'read' -and $Body.can_approve_pull_request_reviews -eq $false) 'Actions permissions may only be restricted to read-only workflow tokens.'
        } elseif($Path -match '/branches/main/protection$'){
            $branchKeys=@('required_status_checks','enforce_admins','required_pull_request_reviews','restrictions','allow_force_pushes','allow_deletions','required_conversation_resolution','required_linear_history')
            $reviewKeys=@('dismiss_stale_reviews','require_code_owner_reviews','required_approving_review_count')
            Assert (@($Body.Keys|Where-Object{$_ -notin $branchKeys}).Count -eq 0 -and @($Body.required_pull_request_reviews.Keys|Where-Object{$_ -notin $reviewKeys}).Count -eq 0 -and @($Body.required_status_checks.Keys|Where-Object{$_ -notin @('strict','contexts')}).Count -eq 0) 'Branch protection body contains non-allowlisted settings.'
            Assert ($Body.enforce_admins -eq $true -and $Body.required_status_checks.strict -eq $true -and @($Body.required_status_checks.contexts).Count -gt 0 -and $Body.required_pull_request_reviews.required_approving_review_count -eq 0 -and $Body.required_pull_request_reviews.dismiss_stale_reviews -eq $true -and $Body.required_pull_request_reviews.require_code_owner_reviews -eq $false -and $Body.allow_force_pushes -eq $false -and $Body.allow_deletions -eq $false -and $Body.required_conversation_resolution -eq $true -and $Body.required_linear_history -eq $true) 'Branch protection body violates the safe required-check policy.'
        } elseif($Method -eq 'PUT' -and $Body.Count -gt 0){
            Assert ($Body.Count -eq 0) 'Security feature mutations must not include object payloads.'
        }
    }
    Assert $allowed "Blocked non-allowlisted GitHub request: $Method $Path"
}
function Invoke-Api([string] $Method, [string] $Path, $Body = $null) {
    Assert-AllowedRequest $Method $Path $Body
    $args = @('api', '--method', $Method, $Path)
    $inputText = ''
    if ($null -ne $Body) { $args += @('--input', '-'); $inputText = Json $Body }
    $raw = Invoke-Gh $args $inputText
    if ([string]::IsNullOrWhiteSpace($raw)) { return $null }
    return ConvertFrom-Json -InputObject $raw -AsHashtable
}
function Invoke-GraphQL([string] $Query, [hashtable] $Variables = @{}) {
    $payload = @{ query = $Query; variables = $Variables }
    $data = Invoke-Api 'POST' 'graphql' $payload
    if ($data.errors) { throw "GraphQL error: $((@($data.errors | ForEach-Object { $_.message }) -join '; '))" }
    if($Query -match '\{\s*createProjectV2View\b'){$newViewId=[string]$data.data.createProjectV2View.projectV2View.id;if($newViewId){[void]$script:ProjectViewIds.Add($newViewId);[void]$script:AllowedProjectViewIds.Add($newViewId)}}
    return $data.data
}
function Add-Operation([string] $Name, [string] $Target, $Desired, $Observed, [scriptblock] $Action) {
    $equal = (Canonical $Desired) -ceq (Canonical $Observed)
    $entry = [ordered]@{ name = $Name; target = $Target; state = if ($equal) { 'verified' } else { 'pending' }; desired = $Desired; observed = $Observed }
    [void] $script:Operations.Add($entry)
    if (-not $equal -and $Apply) {
        if ($VerifyOnly) { throw '-Apply and -VerifyOnly cannot be used together.' }
        Start-Sleep -Milliseconds $script:DelayMilliseconds
        $script:Changed = $true
        $entry.state = 'mutation-attempted'
        try{& $Action;$entry.state = 'applied-awaiting-refetch'}catch{$entry.state='mutation-failed';$entry.error=$_.Exception.Message;[void]$script:Failures.Add("$Name mutation failed at ${Target}: $($_.Exception.Message)")}
    }
}
function Refetch-Verify([string] $Name, [string] $Target, $Desired, [scriptblock] $Fetch) {
    if (-not $Apply) { return }
    $entry = $script:Operations | Where-Object { $_.name -eq $Name -and $_.target -eq $Target } | Select-Object -Last 1
    try{$actual = & $Fetch}catch{$entry.state='refetch-failed';$entry.error=$_.Exception.Message;[void]$script:Failures.Add("$Name refetch failed at ${Target}: $($_.Exception.Message)");return}
    $entry.observedAfter = $actual
    $entry.state = if ((Canonical $Desired) -ceq (Canonical $actual)) { 'verified' } else { 'failed-verification' }
    if ($entry.state -ne 'verified') { [void] $script:Failures.Add("$Name did not match after refetch at ${Target}") }
}
function Graph([string] $Query, [hashtable] $Vars = @{}) { Invoke-GraphQL $Query $Vars }
function Get-Project {
    $query = 'query($id:ID!){node(id:$id){... on ProjectV2{id title url public items(first:1){totalCount} repositories(first:100){nodes{id nameWithOwner}} fields(first:100){nodes{__typename ... on ProjectV2FieldCommon{id name dataType} ... on ProjectV2SingleSelectField{options{id name color description}} ... on ProjectV2MultiSelectField{multiSelectOptions{id name color description}}}} views(first:100){nodes{id name layout filter configuration{visibleFields(first:100){nodes{... on ProjectV2FieldCommon{id name}}}} groupByFields(first:100){nodes{... on ProjectV2FieldCommon{id name}}} verticalGroupByFields(first:100){nodes{... on ProjectV2FieldCommon{id name}}} sortByFields(first:100){nodes{field{... on ProjectV2FieldCommon{id name}} direction}}}}}}}'
    $project=(Graph $query @{ id = $script:ProjectId }).node
    Assert ($project.public -eq $false) 'Project visibility differs from the private configuration baseline; refusing mutation.'
    foreach ($field in $project.fields.nodes) {
        if ($field.dataType -eq 'MULTI_SELECT') { $field.options = @($field.multiSelectOptions) }
    }
    foreach ($view in $project.views.nodes) {
        $view.configuration.visibleFieldIds = @($view.configuration.visibleFields.nodes | ForEach-Object id)
        # Keep the native vectors separate: TABLE groupByFields is primary;
        # BOARD verticalGroupByFields is primary and groupByFields is secondary.
        $view.groupByFields = @($view.groupByFields.nodes)
        $view.verticalGroupByFields = @($view.verticalGroupByFields.nodes)
        $view.sortByFields = @($view.sortByFields.nodes)
    }
    $script:ProjectFieldIds=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:ProjectViewIds=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:AllowedProjectFieldIds=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $script:AllowedProjectViewIds=[Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach($field in $project.fields.nodes){[void]$script:ProjectFieldIds.Add([string]$field.id);if($script:ProjectFieldKinds[$field.name] -in @('SINGLE_SELECT','MULTI_SELECT')){[void]$script:AllowedProjectFieldIds.Add([string]$field.id)}}
    foreach($view in $project.views.nodes){[void]$script:ProjectViewIds.Add([string]$view.id);if($view.name -in $script:ProjectViewNames){[void]$script:AllowedProjectViewIds.Add([string]$view.id)}}
    if('Master Roadmap' -notin @($project.views.nodes.name)){foreach($seed in @($project.views.nodes|Where-Object{$_.name -in @('View1','View 1')})){[void]$script:AllowedProjectViewIds.Add([string]$seed.id)}}
    return $project
}
function Get-Label([string] $RepositorySlug, [string] $Name) {
    try { Invoke-Api 'GET' "repos/$RepositorySlug/labels/$([Uri]::EscapeDataString($Name))" }
    catch { if ($_.Exception.Message -match '(?i)(HTTP 404|status code 404|Not Found)') { return $null }; throw }
}
function Get-Milestone([string] $RepositorySlug, [string] $Title) {
    foreach ($state in @('open','closed')) {
        for($page=1;$page -le 10;$page++){
            $items = Invoke-Api 'GET' "repos/$RepositorySlug/milestones?state=$state&per_page=100&page=$page"
            $found = @($items | Where-Object { $_.title -ceq $Title })
            if ($found.Count) { return $found[0] }
            if(@($items).Count -lt 100){break}
        }
    }
    return $null
}
function Set-Milestone($Milestone) {
    $targetRepo = if ($Milestone.repository -eq 'asciiwall-library') { $script:LibrarySlug } else { $script:RepoSlug }
    $existing = Get-Milestone $targetRepo $Milestone.title
    $description="Goal: $($Milestone.goal)`nEntry: $(@($Milestone.entry_conditions) -join '; ')`nExit: $(@($Milestone.exit_conditions) -join '; ')`nGate ID: $($Milestone.gate)`nEvidence plan: $($Milestone.quality_gate)`nSign-off: $(@($Milestone.signoff) -join '; ')`nPlanned release: $($Milestone.target_release)`nPlanned effort: $($Milestone.effort_days) estimated engineering days; planning only. No dates or completion claims."
    $desired = @{ title = $Milestone.title; description = $description }
    $observed = if ($existing) { @{title=$existing.title;description=$existing.description} } else { $null }
    $path = if ($existing) { "repos/$targetRepo/milestones/$($existing.number)" } else { "repos/$targetRepo/milestones" }
    $method = if ($existing) { 'PATCH' } else { 'POST' }
    $key = "milestone:$($Milestone.id)"
    Add-Operation $key $targetRepo $desired $observed {Invoke-Api $method $path $desired|Out-Null}
    $operation=$script:Operations|Where-Object{$_.name -eq $key -and $_.target -eq $targetRepo}|Select-Object -Last 1
    if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){Refetch-Verify $key $targetRepo $desired {$m=Get-Milestone $targetRepo $Milestone.title;if($m){@{title=$m.title;description=$m.description}}else{$null}}}
}
function Field-Id($Project, [string] $Name) { @($Project.fields.nodes | Where-Object { $_.name -ceq $Name }) | Select-Object -First 1 }
function Set-ProjectField($Project, $Definition) {
    $key="field:$($Definition.name)"
    $existing=Field-Id $Project $Definition.name
    $expectedType=switch($Definition.kind){
        SINGLE_SELECT {'SINGLE_SELECT'} MULTI_SELECT {'MULTI_SELECT'} TEXT {'TEXT'} NUMBER {'NUMBER'}
        NATIVE_REPOSITORY {'REPOSITORY'} NATIVE_MILESTONE {'MILESTONE'} NATIVE_ASSIGNEES {'ASSIGNEES'}
        NATIVE_TITLE {'TITLE'} NATIVE_LABELS {'LABELS'} NATIVE_PARENT_ISSUE {'PARENT_ISSUE'}
        NATIVE_SUB_ISSUES_PROGRESS {'SUB_ISSUES_PROGRESS'} NATIVE_UPDATED {'UPDATED'}
        default {throw "Unsupported project field kind '$($Definition.kind)'."}
    }
    if(-not $existing){
        if($Definition.kind -in @('NATIVE_REPOSITORY','NATIVE_MILESTONE','NATIVE_ASSIGNEES','NATIVE_TITLE','NATIVE_LABELS','NATIVE_PARENT_ISSUE','NATIVE_SUB_ISSUES_PROGRESS','NATIVE_UPDATED')){
            [void]$script:Failures.Add("Required native project field '$($Definition.name)' ($expectedType) is absent; a custom shadow field will not be created.")
            [void]$script:Operations.Add([pscustomobject]@{name=$key;target=$script:ProjectId;state='missing-native-field';desired=@{name=$Definition.name;kind=$expectedType};observed=$null})
            return $Project
        }
        $isSingle=$Definition.kind -eq 'SINGLE_SELECT'
        $isMulti=$Definition.kind -eq 'MULTI_SELECT'
        if ($isSingle) {
            $query='mutation($project:ID!,$name:String!,$options:[ProjectV2SingleSelectFieldOptionInput!]!){createProjectV2Field(input:{projectId:$project,dataType:SINGLE_SELECT,name:$name,singleSelectOptions:$options}){projectV2Field{... on ProjectV2FieldCommon{id}}}}'
        } elseif ($isMulti) {
            $query='mutation($project:ID!,$name:String!,$options:[ProjectV2MultiSelectFieldOptionInput!]!){createProjectV2Field(input:{projectId:$project,dataType:MULTI_SELECT,name:$name,multiSelectOptions:$options}){projectV2Field{... on ProjectV2FieldCommon{id}}}}'
        } else {
            $query='mutation($project:ID!,$name:String!,$type:ProjectV2CustomFieldType!){createProjectV2Field(input:{projectId:$project,dataType:$type,name:$name}){projectV2Field{... on ProjectV2FieldCommon{id}}}}'
        }
        $options=@($Definition.options|ForEach-Object{@{name=$_;color='GRAY';description=''}})
        $desired=@{name=$Definition.name;kind=$expectedType};if($isSingle -or $isMulti){$desired.options=@($Definition.options)}
        Add-Operation $key $script:ProjectId $desired $null {
            if($isSingle -or $isMulti){Graph $query @{project=$script:ProjectId;name=$Definition.name;options=$options}|Out-Null}
            else{Graph $query @{project=$script:ProjectId;name=$Definition.name;type=$expectedType}|Out-Null}
        }
        $operation=$script:Operations|Where-Object{$_.name -eq $key -and $_.target -eq $script:ProjectId}|Select-Object -Last 1
        if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){
            $latest=Get-Project;$actualField=Field-Id $latest $Definition.name
            $createdObserved=@{name=[string]$actualField.name;kind=[string]$actualField.dataType};if($isSingle -or $isMulti){$createdObserved.options=@($actualField.options|ForEach-Object name)}
            Refetch-Verify $key $script:ProjectId $desired {$createdObserved}
            $Project=$latest
        }
        return $Project
    }
    $observedType=[string]$existing.dataType
    if($observedType -ne $expectedType){
        [void]$script:Failures.Add("Project field '$($Definition.name)' has type '$observedType', expected '$expectedType'.")
        [void]$script:Operations.Add([pscustomobject]@{name=$key;target=$script:ProjectId;state='field-type-mismatch';desired=@{name=$Definition.name;kind=$expectedType};observed=@{name=$existing.name;kind=$observedType}})
        return $Project
    }
    if($Definition.kind -notin @('SINGLE_SELECT','MULTI_SELECT')){
        [void]$script:Operations.Add([pscustomobject]@{name=$key;target=$script:ProjectId;state='verified';desired=@{name=$Definition.name;kind=$expectedType};observed=@{name=$existing.name;kind=$observedType}})
        return $Project
    }
    $actualOptions=@($existing.options|ForEach-Object{@{id=[string]$_.id;name=[string]$_.name;color=$_.color;description=$_.description}})
    $optionMap=[Collections.Generic.List[object]]::new()
    foreach($option in $actualOptions){[void]$optionMap.Add(@{id=$option.id;name=$option.name;color=$option.color;description=$option.description})}
    $remoteNames=@($actualOptions|ForEach-Object name)
    $renameTodo=$Definition.name -ceq 'Status' -and $Project.items.totalCount -eq 0 -and 'Todo' -cin $remoteNames -and 'Backlog' -cnotin $remoteNames
    if($renameTodo){foreach($option in $optionMap){if($option.name -ceq 'Todo'){$option.name='Backlog'}};$remoteNames=@($optionMap|ForEach-Object name)}
    foreach($name in $Definition.options){if($name -cnotin $remoteNames){[void]$optionMap.Add(@{name=$name;color='GRAY';description=''});$remoteNames+=@($name)}}
    $desiredOptions=@(foreach($name in $Definition.options){$optionMap|Where-Object name -ceq $name};$optionMap|Where-Object{$_.name -cnotin $Definition.options})
    $observed=@{name=[string]$existing.name;kind=$observedType;options=$actualOptions}
    $desired=@{name=$Definition.name;kind=$expectedType;options=$desiredOptions}
    if ($Definition.kind -eq 'SINGLE_SELECT') {
        $mutation='mutation($field:ID!,$options:[ProjectV2SingleSelectFieldOptionInput!]!){updateProjectV2Field(input:{fieldId:$field,singleSelectOptions:$options}){projectV2Field{... on ProjectV2FieldCommon{id}}}}'
    } else {
        $mutation='mutation($field:ID!,$options:[ProjectV2MultiSelectFieldOptionInput!]!){updateProjectV2Field(input:{fieldId:$field,multiSelectOptions:$options}){projectV2Field{... on ProjectV2FieldCommon{id}}}}'
    }
    $apiOptions=@($desiredOptions|ForEach-Object{$entry=@{name=$_.name;color=$_.color;description=$_.description};if($_.id){$entry.id=$_.id};$entry})
    Add-Operation $key $script:ProjectId $desired $observed {Graph $mutation @{field=$existing.id;options=$apiOptions}|Out-Null}
    if($Definition.name -eq 'Status' -and 'Todo' -in @($actualOptions|ForEach-Object name) -and $Project.items.totalCount -gt 0){$migration="Status migration: the project has $($Project.items.totalCount) item(s); legacy Todo was preserved and existing items must be migrated separately.";$script:PendingManual.Add($migration);[void]$script:Operations.Add([pscustomobject]@{name='status-migration:Todo-to-Backlog';target=$script:ProjectId;state='pending-manual';desired='Migrate existing Todo items only with separate authorization.';observed=@{item_count=$Project.items.totalCount;Todo_preserved=$true}})}
    $operation=$script:Operations|Where-Object{$_.name -eq $key -and $_.target -eq $script:ProjectId}|Select-Object -Last 1
    if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){
        $latest=Get-Project;$actualField=Field-Id $latest $Definition.name
        $expectedOptionIds=@($desiredOptions|Where-Object{$_.id}|ForEach-Object{[string]$_.id})
        Refetch-Verify $key $script:ProjectId $desired {$verifiedOptions=@($actualField.options|ForEach-Object{$entry=@{name=[string]$_.name;color=$_.color;description=$_.description};if(([string]$_.id) -in $expectedOptionIds){$entry.id=[string]$_.id};$entry});@{name=[string]$actualField.name;kind=[string]$actualField.dataType;options=$verifiedOptions}}
        $Project=$latest
    }
    return $Project
}
function Set-Label([string] $RepositorySlug, $Label) {
    $existing = Get-Label $RepositorySlug $Label.name
    $desired = @{ name = $Label.name; color = $Label.color.ToLowerInvariant(); description = $Label.description }
    $observed = if ($existing) { @{ name = $existing.name; color = $existing.color.ToLowerInvariant(); description = $existing.description } } else { $null }
    $path = if ($existing) { "repos/$RepositorySlug/labels/$([Uri]::EscapeDataString($Label.name))" } else { "repos/$RepositorySlug/labels" }
    $method = if ($existing) { 'PATCH' } else { 'POST' }
    $key = "label:$($Label.name)"
    Add-Operation $key $RepositorySlug $desired $observed { Invoke-Api $method $path $desired | Out-Null }
    $operation=$script:Operations|Where-Object{$_.name -eq $key -and $_.target -eq $RepositorySlug}|Select-Object -Last 1
    if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){Refetch-Verify $key $RepositorySlug $desired {$l=Get-Label $RepositorySlug $Label.name;@{name=$l.name;color=$l.color.ToLowerInvariant();description=$l.description}}}
}
function Set-ProjectView($Project, $View) {
    $existing=@($Project.views.nodes|Where-Object name -ceq $View.name|Select-Object -First 1)[0]
    if(-not $existing -and $View.name -eq 'Master Roadmap'){$existing=@($Project.views.nodes|Where-Object{$_.name -in @('View1','View 1')}|Select-Object -First 1)[0]}
    $fieldMap=@{};foreach($f in $Project.fields.nodes){$fieldMap[$f.name]=$f.id}
    $missing=@($View.visible_fields|Where-Object{-not $fieldMap.ContainsKey($_)})
    if($missing.Count){[void]$script:Failures.Add("View '$($View.name)' references unknown fields: $($missing -join ', ').");[void]$script:Operations.Add([pscustomobject]@{name="view:$($View.name)";target=$script:ProjectId;state='missing-visible-fields';desired=$View.visible_fields;observed=@($fieldMap.Keys)});return}
    $ids=@($View.visible_fields|ForEach-Object{$fieldMap[$_]}|Where-Object{$_})
    $layout=[string]$View.layout
    $desired=@{name=$View.name;layout=$layout;filter=[string]$View.filter;visibleFieldIds=$ids}
    $observed=if($existing){@{name=$existing.name;layout=$existing.layout;filter=[string]$existing.filter;visibleFieldIds=@($existing.configuration.visibleFieldIds)}}else{$null}
    $key="view:$($View.name)"
    Add-Operation $key $script:ProjectId $desired $observed {
        $config=@{visibleFieldIds=$ids}
        if($existing){$mutation='mutation($id:ID!,$name:String!,$filter:String!,$layout:ProjectV2ViewLayout!,$config:ProjectV2ViewConfigurationInput!){updateProjectV2View(input:{viewId:$id,name:$name,filter:$filter,layout:$layout,configuration:$config}){projectV2View{id}}}';Graph $mutation @{id=$existing.id;name=$View.name;filter=[string]$View.filter;layout=$layout;config=$config}|Out-Null}
        else {
            $createMutation='mutation($project:ID!,$name:String!,$layout:ProjectV2ViewLayout!,$config:ProjectV2ViewConfigurationInput!){createProjectV2View(input:{projectId:$project,name:$name,layout:$layout,configuration:$config}){projectV2View{id}}}'
            $created=Graph $createMutation @{project=$script:ProjectId;name=$View.name;layout=$layout;config=$config}
            $viewId=[string]$created.createProjectV2View.projectV2View.id
            Assert ([bool]$viewId) "GitHub did not return an id for new view '$($View.name)'."
            [void]$script:ProjectViewIds.Add($viewId)
            Start-Sleep -Milliseconds $script:DelayMilliseconds
            $updateMutation='mutation($id:ID!,$name:String!,$filter:String!,$layout:ProjectV2ViewLayout!,$config:ProjectV2ViewConfigurationInput!){updateProjectV2View(input:{viewId:$id,name:$name,filter:$filter,layout:$layout,configuration:$config}){projectV2View{id}}}'
            Graph $updateMutation @{id=$viewId;name=$View.name;filter=[string]$View.filter;layout=$layout;config=$config}|Out-Null
        }
    }
    $operation=$script:Operations|Where-Object{$_.name -eq $key -and $_.target -eq $script:ProjectId}|Select-Object -Last 1
    if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){Refetch-Verify $key $script:ProjectId $desired {$latest=Get-Project;$v=@($latest.views.nodes|Where-Object name -ceq $View.name|Select-Object -First 1)[0];@{name=$v.name;layout=$v.layout;filter=[string]$v.filter;visibleFieldIds=@($v.configuration.visibleFieldIds)}}}
    if($layout -eq 'BOARD_LAYOUT'){
        $primaryField=$View.group_by;$secondaryField=$View.secondary_group_by
        $actualPrimary=@(if($existing){$existing.verticalGroupByFields|ForEach-Object{$_.name}})
        $actualSecondary=@(if($existing){$existing.groupByFields|ForEach-Object{$_.name}})
    }else{
        $primaryField=$View.group_by;$secondaryField=$null
        $actualPrimary=@(if($existing){$existing.groupByFields|ForEach-Object{$_.name}})
        $actualSecondary=@(if($existing){$existing.verticalGroupByFields|ForEach-Object{$_.name}})
    }
    $expectedSort=@($View.sort_by)
    $actualSort=@(if($existing){$existing.sortByFields|ForEach-Object{$_.field.name}})
    $secondaryExpected=$null
    if($secondaryField){$secondaryExpected=@($secondaryField)}
    if($layout -eq 'TABLE_LAYOUT' -and $existing -and $actualSecondary.Count -eq 0){$actualSecondary=$null}
    $apiDesired=@{primary_group_by=@($primaryField);secondary_group_by=$secondaryExpected;sort_by=$expectedSort}
    $apiObserved=@{primary_group_by=$actualPrimary;secondary_group_by=$actualSecondary;sort_by=$actualSort}
    $apiState=if($existing -and (Canonical-OrderedVector $apiDesired.primary_group_by) -ceq (Canonical-OrderedVector $apiObserved.primary_group_by) -and (Canonical-OrderedVector $apiDesired.secondary_group_by) -ceq (Canonical-OrderedVector $apiObserved.secondary_group_by) -and (Canonical-OrderedVector $apiDesired.sort_by) -ceq (Canonical-OrderedVector $apiObserved.sort_by)){'verified'}else{'pending'}
    [void]$script:Operations.Add([pscustomobject]@{name="view-api:$($View.name)";target=$script:ProjectId;state=$apiState;desired=$apiDesired;observed=$apiObserved})
    if($apiState -ne 'verified'){$script:PendingManual.Add("View '$($View.name)': native API grouping or ordered sort fields differ.")}
    if($View.slice_by){
        [void]$script:Operations.Add([pscustomobject]@{name="view-slice:$($View.name)";target=$script:ProjectId;state='manual-only-unverifiable-by-api';desired=@{slice_by=$View.slice_by};observed=$null;verification='Must be checked manually in the authorized GitHub UI after save and reload; no public API read/write exposes Slice by.'})
        $script:PendingManual.Add("View '$($View.name)': manually verify Slice by '$($View.slice_by)' in the authorized GitHub UI after saving and reloading; no public API can verify this setting.")
    }
}
function Get-SecurityEnabled([string] $RepositorySlug,[string] $Feature,$RepositoryState) {
    if($Feature -eq 'secret_scanning'){return $RepositoryState.security_and_analysis.secret_scanning.status -eq 'enabled'}
    if($Feature -eq 'secret_scanning_push_protection'){return $RepositoryState.security_and_analysis.secret_scanning_push_protection.status -eq 'enabled'}
    $path=switch($Feature){dependabot_alerts{"repos/$RepositorySlug/vulnerability-alerts"}dependabot_security_updates{"repos/$RepositorySlug/automated-security-fixes"}private_vulnerability_reporting{"repos/$RepositorySlug/private-vulnerability-reporting"}}
    try{
        $result=Invoke-Api 'GET' $path
        if($Feature -eq 'dependabot_alerts'){return $true}
        return [bool]$result.enabled
    }catch{
        if($_.Exception.Message -match '(?i)(HTTP 404|status code 404|Not Found)'){return $false}
        throw
    }
}
function Set-RepositorySettings([string] $RepositorySlug,[string] $Description,[string[]] $DesiredTopics) {
    $current=Invoke-Api 'GET' "repos/$RepositorySlug"
    Assert ($current.private -eq $false) "Repository $RepositorySlug is not public; refusing configuration."
    $desired=@{description=$Description;has_discussions=$true;allow_squash_merge=$true;allow_merge_commit=$false;allow_rebase_merge=$false;delete_branch_on_merge=$true;allow_forking=$current.allow_forking;web_commit_signoff_required=$false;has_issues=$current.has_issues}
    $observed=@{description=$current.description;has_discussions=$current.has_discussions;allow_squash_merge=$current.allow_squash_merge;allow_merge_commit=$current.allow_merge_commit;allow_rebase_merge=$current.allow_rebase_merge;delete_branch_on_merge=$current.delete_branch_on_merge;allow_forking=$current.allow_forking;web_commit_signoff_required=$current.web_commit_signoff_required;has_issues=$current.has_issues}
    $patch=@{description=$Description;has_discussions=$true;allow_squash_merge=$true;allow_merge_commit=$false;allow_rebase_merge=$false;delete_branch_on_merge=$true;web_commit_signoff_required=$false}
    $key="repository:settings"
    Add-Operation $key $RepositorySlug $desired $observed {Invoke-Api 'PATCH' "repos/$RepositorySlug" $patch|Out-Null}
    $operation=$script:Operations|Where-Object{$_.name -eq $key -and $_.target -eq $RepositorySlug}|Select-Object -Last 1
    if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){Refetch-Verify $key $RepositorySlug $desired {$r=Invoke-Api 'GET' "repos/$RepositorySlug";@{description=$r.description;has_discussions=$r.has_discussions;allow_squash_merge=$r.allow_squash_merge;allow_merge_commit=$r.allow_merge_commit;allow_rebase_merge=$r.allow_rebase_merge;delete_branch_on_merge=$r.delete_branch_on_merge;allow_forking=$r.allow_forking;web_commit_signoff_required=$r.web_commit_signoff_required;has_issues=$r.has_issues}}}
    $topicObserved=@((Invoke-Api 'GET' "repos/$RepositorySlug/topics").names)
    $wanted=@(@($topicObserved)+@($DesiredTopics)|Sort-Object -Unique)
    $topicBody=@{names=$wanted}
    Add-Operation 'repository:topics' $RepositorySlug $wanted @($topicObserved|Sort-Object -Unique) {Invoke-Api 'PUT' "repos/$RepositorySlug/topics" $topicBody|Out-Null}
    $operation=$script:Operations|Where-Object{$_.name -eq 'repository:topics' -and $_.target -eq $RepositorySlug}|Select-Object -Last 1
    if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){Refetch-Verify 'repository:topics' $RepositorySlug $wanted {@((Invoke-Api 'GET' "repos/$RepositorySlug/topics").names|Sort-Object -Unique)}}
    $repoState=Invoke-Api 'GET' "repos/$RepositorySlug"
    foreach($feature in @('secret_scanning','secret_scanning_push_protection','dependabot_alerts','dependabot_security_updates','private_vulnerability_reporting')){
        $enabled=Get-SecurityEnabled $RepositorySlug $feature $repoState
        $path=switch($feature){dependabot_alerts{"repos/$RepositorySlug/vulnerability-alerts"}dependabot_security_updates{"repos/$RepositorySlug/automated-security-fixes"}private_vulnerability_reporting{"repos/$RepositorySlug/private-vulnerability-reporting"}}
        $repoPatch=$null
        if($feature -in @('secret_scanning','secret_scanning_push_protection')){$securityKey=if($feature -eq 'secret_scanning'){'secret_scanning'}else{'secret_scanning_push_protection'};$repoPatch=@{security_and_analysis=@{$securityKey=@{status='enabled'}}}}
        $mutation=if($null -ne $repoPatch){ {Invoke-Api 'PATCH' "repos/$RepositorySlug" $repoPatch|Out-Null} } else { {Invoke-Api 'PUT' $path @{}|Out-Null} }
        Add-Operation "security:$feature" $RepositorySlug @{enabled=$true} @{enabled=$enabled} $mutation
        $op="security:$feature"
        $operation=$script:Operations|Where-Object{$_.name -eq $op -and $_.target -eq $RepositorySlug}|Select-Object -Last 1
        if($Apply -and $operation.state -eq 'applied-awaiting-refetch'){Refetch-Verify $op $RepositorySlug @{enabled=$true} {$state=Invoke-Api 'GET' "repos/$RepositorySlug";@{enabled=(Get-SecurityEnabled $RepositorySlug $feature $state)}}}
    }
    $workflowPath="repos/$RepositorySlug/actions/permissions/workflow"
    $workflowDesired=@{default_workflow_permissions='read';can_approve_pull_request_reviews=$false}
    $workflowObserved=Invoke-Api 'GET' $workflowPath
    $workflowActual=@{default_workflow_permissions=$workflowObserved.default_workflow_permissions;can_approve_pull_request_reviews=$workflowObserved.can_approve_pull_request_reviews}
    Add-Operation 'actions:workflow-permissions' $RepositorySlug $workflowDesired $workflowActual {Invoke-Api 'PUT' $workflowPath $workflowDesired|Out-Null}
    $workflowOperation=$script:Operations|Where-Object{$_.name -eq 'actions:workflow-permissions' -and $_.target -eq $RepositorySlug}|Select-Object -Last 1
    if($Apply -and $workflowOperation.state -eq 'applied-awaiting-refetch'){Refetch-Verify 'actions:workflow-permissions' $RepositorySlug $workflowDesired {$w=Invoke-Api 'GET' $workflowPath;@{default_workflow_permissions=$w.default_workflow_permissions;can_approve_pull_request_reviews=$w.can_approve_pull_request_reviews}}}
    $discussion=@((Graph 'query($id:ID!){node(id:$id){... on Repository{discussionCategories(first:20){nodes{name}}}}}' @{id=$current.node_id}).node.discussionCategories.nodes.name)
    $wantedCategories=@('Announcements','General','Ideas','Q&A')
    $missing=@($wantedCategories|Where-Object{$_ -notin $discussion})
    $categoryState=if($missing.Count){'pending-manual'}else{'verified'}
    [void]$script:Operations.Add([pscustomobject]@{name='repository:discussion-categories';target=$RepositorySlug;state=$categoryState;desired=$wantedCategories;observed=$discussion})
    if($missing.Count){$script:PendingManual.Add("$RepositorySlug discussion categories require GitHub interactive settings: $($missing -join ', ').")}
}
function Set-BranchProtection([string] $RepositorySlug) {
    if(-not $ConfigureBranchProtection){return}
    Assert ($RequiredCheck.Count -gt 0 -and @($RequiredCheck|Where-Object{[string]::IsNullOrWhiteSpace($_)}).Count -eq 0) 'Branch protection requires nonempty explicit -RequiredCheck values published by the parent after green CI.'
    $branch=$null
    try{$branch=Invoke-Api 'GET' "repos/$RepositorySlug/branches/main/protection"}
    catch{if($_.Exception.Message -notmatch '(?i)(HTTP 404|status code 404|Not Found)'){throw}}
    $required=@(@($RequiredCheck)+@($branch.required_status_checks.contexts)|Where-Object{$_}|Sort-Object -Unique)
    $normalized=@{required_status_checks=@{strict=$true;contexts=$required};enforce_admins=$true;required_approving_review_count=0;dismiss_stale_reviews=$true;require_code_owner_reviews=$false;allow_force_pushes=$false;allow_deletions=$false;required_conversation_resolution=$true;required_linear_history=$true}
    $restrictions = if ($branch.restrictions) { @{users=@($branch.restrictions.users.login);teams=@($branch.restrictions.teams.slug);apps=@($branch.restrictions.apps.slug)} } else { $null }
    $body=@{required_status_checks=@{strict=$true;contexts=$required};enforce_admins=$true;required_pull_request_reviews=@{dismiss_stale_reviews=$true;require_code_owner_reviews=$false;required_approving_review_count=0};restrictions=$restrictions;allow_force_pushes=$false;allow_deletions=$false;required_conversation_resolution=$true;required_linear_history=$true}
    $observed=$null
    if($branch){$observed=@{required_status_checks=@{strict=$branch.required_status_checks.strict;contexts=@($branch.required_status_checks.contexts|Sort-Object -Unique)};enforce_admins=$branch.enforce_admins.enabled;required_approving_review_count=$branch.required_pull_request_reviews.required_approving_review_count;dismiss_stale_reviews=$branch.required_pull_request_reviews.dismiss_stale_reviews;require_code_owner_reviews=$branch.required_pull_request_reviews.require_code_owner_reviews;allow_force_pushes=$branch.allow_force_pushes.enabled;allow_deletions=$branch.allow_deletions.enabled;required_conversation_resolution=$branch.required_conversation_resolution.enabled;required_linear_history=$branch.required_linear_history.enabled}}
    $key='branch-protection:main'
    Add-Operation $key $RepositorySlug $normalized $observed {Invoke-Api 'PUT' "repos/$RepositorySlug/branches/main/protection" $body|Out-Null}
    if($Apply){Refetch-Verify $key $RepositorySlug $normalized {$b=Invoke-Api 'GET' "repos/$RepositorySlug/branches/main/protection";@{required_status_checks=@{strict=$b.required_status_checks.strict;contexts=@($b.required_status_checks.contexts|Sort-Object -Unique)};enforce_admins=$b.enforce_admins.enabled;required_approving_review_count=$b.required_pull_request_reviews.required_approving_review_count;dismiss_stale_reviews=$b.required_pull_request_reviews.dismiss_stale_reviews;require_code_owner_reviews=$b.required_pull_request_reviews.require_code_owner_reviews;allow_force_pushes=$b.allow_force_pushes.enabled;allow_deletions=$b.allow_deletions.enabled;required_conversation_resolution=$b.required_conversation_resolution.enabled;required_linear_history=$b.required_linear_history.enabled}}}
}

Assert (-not ($Apply -and $VerifyOnly)) '-Apply and -VerifyOnly are mutually exclusive.'
$reportPath=Assert-ExternalReportPath $ReportPath
$report=[ordered]@{mode=if($VerifyOnly){'verify-only'}elseif($Apply){'apply'}else{'offline-plan'};repository=$script:RepoSlug;project_id='';started_at=[DateTime]::UtcNow.ToString('o');operations=@();manual_steps=@();failures=@();visibility_unchanged=$true;issue_publication_attempted=$false}
try{
    $labels=Read-Json (Join-Path $script:Definitions 'labels.json')
    $milestones=Read-Json (Join-Path $script:Definitions 'milestones.json')
    $definition=Read-Json (Join-Path $script:Definitions 'project.json')
    Validate-Definitions $labels $milestones $definition
    $script:ProjectId=[string]$definition.project.id
$script:DesiredProjectTitle=[string]$definition.project.desired_title
$script:ProjectFieldNames=@($definition.fields.name)
$script:ProjectFieldKinds=@{};foreach($fieldDefinition in $definition.fields){$script:ProjectFieldKinds[$fieldDefinition.name]=$fieldDefinition.kind}
$script:ProjectViewNames=@($definition.views.name)
    $report.project_id=$script:ProjectId
    $repoDefinitions=@(
        @{slug=$script:RepoSlug;name=$Repository;description='Planning repository for the proposed cross-platform AsciiWall application and Rust engine.';topics=@('ascii-art','wallpaper','rust','cross-platform')}
        @{slug=$script:LibrarySlug;name=$LibraryRepository;description='Repository foundations and planning for a curated ASCII wallpaper library for AsciiWall.';topics=@('ascii-art','wallpaper','content-library')}
    )
    if(-not $Apply -and -not $VerifyOnly){
        foreach($repoDef in $repoDefinitions){
            [void]$script:Operations.Add([pscustomobject]@{name='repository:settings';target=$repoDef.slug;state='planned';desired=@{description=$repoDef.description;has_discussions=$true;allow_squash_merge=$true;allow_merge_commit=$false;allow_rebase_merge=$false;delete_branch_on_merge=$true}})
            [void]$script:Operations.Add([pscustomobject]@{name='repository:topics';target=$repoDef.slug;state='planned';desired=@($repoDef.topics)})
            foreach($feature in @('secret_scanning','secret_scanning_push_protection','dependabot_alerts','dependabot_security_updates','private_vulnerability_reporting')){[void]$script:Operations.Add([pscustomobject]@{name="security:$feature";target=$repoDef.slug;state='planned';desired=@{enabled=$true}})}
            [void]$script:Operations.Add([pscustomobject]@{name='actions:workflow-permissions';target=$repoDef.slug;state='planned';desired=@{default_workflow_permissions='read';can_approve_pull_request_reviews=$false}})
            foreach($label in $labels){[void]$script:Operations.Add([pscustomobject]@{name="label:$($label.name)";target=$repoDef.slug;state='planned';desired=@{name=$label.name;color=$label.color;description=$label.description}})}
        }
        foreach($m in $milestones){$target=if($m.repository -eq $LibraryRepository){$script:LibrarySlug}else{$script:RepoSlug};[void]$script:Operations.Add([pscustomobject]@{name="milestone:$($m.id)";target=$target;state='planned';desired=@{title=$m.title;gate_id=$m.gate;goal=$m.goal;entry=$m.entry_conditions;exit=$m.exit_conditions;evidence_plan=$m.quality_gate;signoff=$m.signoff;target_release=$m.target_release;effort_days=$m.effort_days}})}
        foreach($f in $definition.fields){[void]$script:Operations.Add([pscustomobject]@{name="field:$($f.name)";target=$script:ProjectId;state='planned';desired=@{name=$f.name;kind=$f.kind;options=$f.options}})}
        foreach($v in $definition.views){
            [void]$script:Operations.Add([pscustomobject]@{name="view:$($v.name)";target=$script:ProjectId;state='planned';desired=@{name=$v.name;layout=$v.layout;filter=$v.filter;visible_fields=$v.visible_fields}})
            $plannedSecondary=if($v.secondary_group_by){@($v.secondary_group_by)}else{$null}
            [void]$script:Operations.Add([pscustomobject]@{name="view-api:$($v.name)";target=$script:ProjectId;state='pending';desired=@{primary_group_by=@($v.group_by);secondary_group_by=$plannedSecondary;sort_by=@($v.sort_by)};observed=$null})
            if($v.slice_by){
                [void]$script:Operations.Add([pscustomobject]@{name="view-slice:$($v.name)";target=$script:ProjectId;state='manual-only-unverifiable-by-api';desired=@{slice_by=$v.slice_by};observed=$null;verification='Must be checked manually in the authorized GitHub UI after save and reload; no public API read/write exposes Slice by.'})
                $script:PendingManual.Add("View '$($v.name)': manually verify Slice by '$($v.slice_by)' in the authorized GitHub UI after saving and reloading; no public API can verify this setting.")
            }
        }
        [void]$script:Operations.Add([pscustomobject]@{name='project:title';target=$script:ProjectId;state='planned';desired=$definition.project.desired_title})
        foreach($repoDef in $repoDefinitions){[void]$script:Operations.Add([pscustomobject]@{name="project-link:$($repoDef.slug)";target=$script:ProjectId;state='planned';desired=$repoDef.slug})}
        if($ConfigureBranchProtection){foreach($repoDef in $repoDefinitions){[void]$script:Operations.Add([pscustomobject]@{name='branch-protection:main';target=$repoDef.slug;state='requires-apply-and-required-checks';desired=$RequiredCheck})}}
    }else{
        foreach($repoDef in $repoDefinitions){
            $repo=Invoke-Api 'GET' "repos/$($repoDef.slug)"
            [void]$script:RepositoryNodeIds.Add([string]$repo.node_id)
            Assert ($repo.name -ceq $repoDef.name -and $repo.owner.login -ieq $Owner) "Repository identity mismatch for $($repoDef.slug)."
            Assert ($repo.private -eq $false) "Repository $($repoDef.slug) is not public; refusing configuration."
            Set-RepositorySettings $repoDef.slug $repoDef.description $repoDef.topics
            foreach($label in $labels){Set-Label $repoDef.slug $label}
        }
        foreach($milestone in $milestones){Set-Milestone $milestone}
        $project=Get-Project
        $projectRepos=@($project.repositories.nodes.nameWithOwner)
        foreach($repoDef in $repoDefinitions){
            if($repoDef.slug -notin $projectRepos){
                $repo=Invoke-Api 'GET' "repos/$($repoDef.slug)"
                $key="project-link:$($repoDef.slug)"
                Add-Operation $key $script:ProjectId $true $false {Graph 'mutation($project:ID!,$repo:ID!){linkProjectV2ToRepository(input:{projectId:$project,repositoryId:$repo}){repository{id}}}' @{project=$script:ProjectId;repo=$repo.node_id}|Out-Null}
                if($Apply){Refetch-Verify $key $script:ProjectId $true {$latest=Get-Project;[bool]($repoDef.slug -in @($latest.repositories.nodes.nameWithOwner))}}
            }else{[void]$script:Operations.Add([pscustomobject]@{name="project-link:$($repoDef.slug)";target=$script:ProjectId;state='verified';desired=$repoDef.slug;observed=$repoDef.slug})}
        }
        Add-Operation 'project:title' $script:ProjectId $definition.project.desired_title $project.title {Graph 'mutation($id:ID!,$title:String!){updateProjectV2(input:{projectId:$id,title:$title}){projectV2{id}}}' @{id=$script:ProjectId;title=$definition.project.desired_title}|Out-Null}
        if($Apply){Refetch-Verify 'project:title' $script:ProjectId $definition.project.desired_title {(Get-Project).title}}
        $project=Get-Project
        foreach($field in $definition.fields){$project=Set-ProjectField $project $field}
        foreach($view in $definition.views){Set-ProjectView $project $view}
        foreach($repoDef in $repoDefinitions){Set-BranchProtection $repoDef.slug}
        $script:PendingManual.Add('Native parent/sub-issue and dependency relationships are intentionally not published. No issue, project item, draft item, or issue-metadata operation is allowed.')
    }
}catch{[void]$script:Failures.Add($_.Exception.Message)}
$report.operations=@($script:Operations)
$report.manual_steps=@($script:PendingManual)
if($VerifyOnly){
    foreach($operation in $script:Operations){
        if($operation.state -eq 'verified'){continue}
        if($operation.state -eq 'manual-only-unverifiable-by-api'){$message="Manual-only verification unavailable via API: $($operation.name) cannot be verified through the public API."}
        else{$message="Verification differs or remains pending: $($operation.name) [$($operation.state)]"}
        [void]$script:Failures.Add($message)
    }
}
$report.failures=@($script:Failures)
$report.finished_at=[DateTime]::UtcNow.ToString('o')
$report.mutation_attempted=$script:Changed
$parent=[IO.Path]::GetDirectoryName($reportPath)
if(-not (Test-Path -LiteralPath $parent)){[void][IO.Directory]::CreateDirectory($parent)}
[IO.File]::WriteAllText($reportPath,(ConvertTo-Json -InputObject $report -Depth 100),[Text.UTF8Encoding]::new($false))
Write-Output "Mode: $($report.mode); operations: $($script:Operations.Count); failures: $($script:Failures.Count); report: $reportPath"
if($script:Failures.Count){exit 1}
