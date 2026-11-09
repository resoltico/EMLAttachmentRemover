# Validate installer scripts with the pinned analyzer without executing them.
param([Parameter(Mandatory = $true)][string]$RequiredVersion)
$ErrorActionPreference = 'Stop'
Import-Module -FullyQualifiedName @{ ModuleName = 'PSScriptAnalyzer'; RequiredVersion = $RequiredVersion }
$findings = @(Invoke-ScriptAnalyzer -Path $PSScriptRoot -Settings (Join-Path $PSScriptRoot 'PSScriptAnalyzerSettings.psd1'))
$findings | Format-Table -AutoSize RuleName, ScriptName, Line, Message
if ($findings.Count) { exit 1 }
