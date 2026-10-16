# Prints the clipboard's file list and Preferred DropEffect as JSON, read the way
# Explorer reads them: {"files":[...],"effect":2} where 2 = cut (move), 5 = copy.
#
#   clip.ps1 -Set "C:.txt|C:.txt" [-Cut]   put files there instead, as another app's copy or cut would
#   clip.ps1 -Text "hello"                       put plain text there
param([string]$Set, [switch]$Cut, [string]$Text)
Add-Type -AssemblyName System.Windows.Forms
if ($Set -or $Text) {
    $o = New-Object System.Windows.Forms.DataObject
    if ($Set) {
        $list = New-Object System.Collections.Specialized.StringCollection
        foreach ($f in $Set.Split("|")) { [void]$list.Add($f) }
        $o.SetFileDropList($list)
        $effect = if ($Cut) { 2 } else { 5 }
        $o.SetData("Preferred DropEffect", (New-Object System.IO.MemoryStream(, [BitConverter]::GetBytes([int]$effect))))
    } else {
        $o.SetText($Text)
    }
    # Copy the data out, so it stays after this process ends, like a real app's.
    [System.Windows.Forms.Clipboard]::SetDataObject($o, $true, 10, 50)
}
$d = [System.Windows.Forms.Clipboard]::GetDataObject()
$files = @()
$effect = 0
if ($d) {
    if ($d.GetDataPresent([System.Windows.Forms.DataFormats]::FileDrop)) {
        $files = @($d.GetData([System.Windows.Forms.DataFormats]::FileDrop))
    }
    if ($d.GetDataPresent("Preferred DropEffect")) {
        $ms = $d.GetData("Preferred DropEffect")
        $b = New-Object byte[] 4
        [void]$ms.Read($b, 0, 4)
        $effect = [BitConverter]::ToInt32($b, 0)
    }
}
@{ files = $files; effect = $effect } | ConvertTo-Json -Compress
