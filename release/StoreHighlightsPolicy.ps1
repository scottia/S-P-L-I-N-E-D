Set-StrictMode -Version Latest

function ConvertTo-BoundedStoreHighlights {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string]$Highlights,
        [Parameter(Mandatory = $true)]
        [string]$Version,
        [ValidateRange(1, 1500)]
        [int]$MaximumCharacters = 1500,
        [string]$RepositoryUrl = "https://github.com/scottia/S-P-L-I-N-E-D"
    )

    $source = $Highlights.Trim()
    $sourceCharacters = $source.Length
    $lines = @($source -split '\r?\n' | Where-Object { -not [String]::IsNullOrWhiteSpace($_) })
    if ($lines.Count -eq 0) {
        throw "Microsoft Store Highlights cannot be empty."
    }

    $heading = $lines[0].Trim()
    $bullets = @($lines | Select-Object -Skip 1)
    foreach ($bullet in $bullets) {
        if (-not $bullet.StartsWith("- ", [StringComparison]::Ordinal)) {
            throw "Microsoft Store Highlights contain unsupported non-bullet content: $bullet"
        }
    }

    if ($sourceCharacters -le $MaximumCharacters) {
        return [pscustomobject][ordered]@{
            Highlights = $source
            SourceCharacters = $sourceCharacters
            FinalCharacters = $sourceCharacters
            Compacted = $false
            CandidateBullets = $bullets.Count
            EmittedBullets = $bullets.Count
            OmittedBullets = 0
            DetailsUrl = $null
        }
    }

    $detailsUrl = "$($RepositoryUrl.TrimEnd('/'))/releases/tag/$Version"
    $detailsBullet = "- Full release details: $detailsUrl"
    $selected = [Collections.Generic.List[string]]::new()
    $selected.Add($heading)

    foreach ($bullet in $bullets) {
        $candidateLines = @($selected) + $bullet + $detailsBullet
        $candidate = $candidateLines -join "`n"
        if ($candidate.Length -gt $MaximumCharacters) {
            break
        }
        $selected.Add($bullet)
    }

    $bounded = (@($selected) + $detailsBullet) -join "`n"
    if ($bounded.Length -gt $MaximumCharacters) {
        throw "Microsoft Store Highlights heading and full-release link exceed the $MaximumCharacters-character limit."
    }

    $emittedBullets = $selected.Count - 1
    return [pscustomobject][ordered]@{
        Highlights = $bounded
        SourceCharacters = $sourceCharacters
        FinalCharacters = $bounded.Length
        Compacted = $true
        CandidateBullets = $bullets.Count
        EmittedBullets = $emittedBullets
        OmittedBullets = $bullets.Count - $emittedBullets
        DetailsUrl = $detailsUrl
    }
}
