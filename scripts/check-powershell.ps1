[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$excluded = @('.git', '.issues', '__pycache__', 'target')
$parseErrors = [System.Collections.Generic.List[string]]::new()

Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.ps1' -ErrorAction Stop |
    Where-Object {
        $relative = [System.IO.Path]::GetRelativePath($root, $_.FullName)
        $parts = $relative -split '[\\/]'
        -not ($parts | Where-Object { $_ -in $excluded })
    } |
    Sort-Object FullName |
    ForEach-Object {
        $tokens = $null
        $errors = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile($_.FullName, [ref]$tokens, [ref]$errors)
        foreach ($errorRecord in $errors) {
            $relative = [System.IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/')
            $parseErrors.Add("${relative}:$($errorRecord.Extent.StartLineNumber):$($errorRecord.Extent.StartColumnNumber): $($errorRecord.Message)")
        }
    }

if ($parseErrors.Count -gt 0) {
    foreach ($parseError in $parseErrors) { [Console]::Error.WriteLine("ERROR $parseError") }
    [Console]::Error.WriteLine("FAIL: PowerShell parser found $($parseErrors.Count) syntax error(s).")
    exit 1
}

$count = @(Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.ps1' -ErrorAction Stop |
    Where-Object {
        $relative = [System.IO.Path]::GetRelativePath($root, $_.FullName)
        $parts = $relative -split '[\\/]'
        -not ($parts | Where-Object { $_ -in $excluded })
    }).Count
Write-Output "PASS: PowerShell parser accepted $count repository script file(s); no scripts were executed."
