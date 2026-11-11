#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'publish-issues.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'Publisher syntax errors prevent the ledger regression.' }
foreach ($statement in $ast.EndBlock.Statements) {
    if ($statement -is [Management.Automation.Language.FunctionDefinitionAst] -and $statement.Name -in @('Assert', 'Json', 'Save-Ledger')) {
        . ([scriptblock]::Create($statement.Extent.Text))
    }
}

$directory = Join-Path ([IO.Path]::GetTempPath()) ('asciiwall-ledger-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($directory)
$script:Apply = $true
$script:Utf8 = [Text.UTF8Encoding]::new($false)
$script:LedgerPath = Join-Path $directory 'publication-ledger.json'
$script:Lock = [IO.FileStream]::new((Join-Path $directory 'publication-ledger.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
try {
    $script:Ledger = @{ tickets = @{ 'app/FIRST' = @{ id = 123; verification = @{ state = 'verified' } } }; operations = @{} }
    Save-Ledger
    $first = [IO.File]::ReadAllText($script:LedgerPath) | ConvertFrom-Json -AsHashtable
    Assert ($first.tickets['app/FIRST'].id -eq 123) 'Initial ledger creation lost the durable issue identity.'

    $script:Ledger.operations['create:app/SECOND'] = @{ state = 'intent'; payload = @{ title = 'Unicode ledger — second issue' } }
    Save-Ledger
    $second = [IO.File]::ReadAllText($script:LedgerPath) | ConvertFrom-Json -AsHashtable
    Assert ($second.tickets['app/FIRST'].verification.state -ceq 'verified' -and $second.operations['create:app/SECOND'].state -ceq 'intent') 'Replacing an existing ledger lost prior verification or new mutation intent.'
    Assert ($second.operations['create:app/SECOND'].payload.title -ceq 'Unicode ledger — second issue') 'Replacing an existing ledger corrupted the exact mutation payload.'

    $script:Ledger.operations['create:app/SECOND'].state = 'succeeded'
    $script:Ledger.tickets['app/SECOND'] = @{ id = 456 }
    Save-Ledger
    $third = [IO.File]::ReadAllText($script:LedgerPath) | ConvertFrom-Json -AsHashtable
    Assert ($third.tickets['app/FIRST'].id -eq 123 -and $third.tickets['app/SECOND'].id -eq 456 -and $third.operations['create:app/SECOND'].state -ceq 'succeeded') 'A subsequent replacement lost the committed receipt or an existing issue identity.'
    Write-Output 'PASS: real ledger creation and repeated overwrite preserve durable identities, mutation payloads, and verification state; no GitHub access.'
} finally {
    $script:Lock.Dispose()
    [IO.Directory]::Delete($directory, $true)
}
