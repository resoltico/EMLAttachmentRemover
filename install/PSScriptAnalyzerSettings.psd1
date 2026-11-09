# PSScriptAnalyzer settings for install.ps1. Keep each exclusion explained.
@{
    Severity     = @('Error', 'Warning')
    ExcludeRules = @(
        # The installer is an interactive console program: Write-Host is its
        # user interface, not pipeline output.
        'PSAvoidUsingWriteHost'
    )
}
