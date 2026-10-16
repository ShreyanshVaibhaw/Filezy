# Restores Recycle Bin items whose name contains -Match. The E2E runner calls it
# with its per-run token, so it can only ever touch files that run created.
param([Parameter(Mandatory)][string]$Match)

if ($Match.Length -lt 6) { throw "refusing a match token shorter than 6 characters" }
$bin = (New-Object -ComObject Shell.Application).Namespace(10)
$n = 0
foreach ($item in @($bin.Items())) {
    if ($item.Name -like "*$Match*") {
        $item.InvokeVerb("undelete")
        $n++
    }
}
"restored $n"
