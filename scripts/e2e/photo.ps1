# Makes a small JPEG carrying tags and a title, or reads a JPEG's tags back,
# through WPF's metadata support: an outside program writing and reading the
# same properties Filezy shows and edits.
#
#   photo.ps1 -Path x.jpg -Tags "red;blue" [-Title "Holiday"]   write (replaces the file)
#   photo.ps1 -Path x.jpg -Read                                print the tags, ";"-separated
param(
    [Parameter(Mandatory)][string]$Path,
    [string]$Tags = "",
    [string]$Title = "",
    [switch]$Read
)

Add-Type -AssemblyName PresentationCore, WindowsBase
$I = [System.Windows.Media.Imaging.BitmapSource]

if ($Read) {
    $fs = [IO.File]::OpenRead($Path)
    try {
        $dec = [System.Windows.Media.Imaging.BitmapDecoder]::Create($fs, 'PreservePixelFormat', 'OnLoad')
        $kw = $dec.Frames[0].Metadata.Keywords
        if ($kw) { ($kw -join ";") } else { "" }
    } finally {
        $fs.Close()
    }
    exit 0
}

$pixels = [byte[]](1..64 | ForEach-Object { 200 })
$bmp = $I::Create(4, 4, 96, 96, [System.Windows.Media.PixelFormats]::Bgr32, $null, $pixels, 16)
$meta = New-Object System.Windows.Media.Imaging.BitmapMetadata 'jpg'
if ($Tags) {
    $list = [string[]]($Tags.Split(';') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    $meta.Keywords = New-Object 'System.Collections.ObjectModel.ReadOnlyCollection[string]' (, $list)
}
if ($Title) { $meta.Title = $Title }
$enc = New-Object System.Windows.Media.Imaging.JpegBitmapEncoder
$enc.Frames.Add([System.Windows.Media.Imaging.BitmapFrame]::Create($bmp, $null, $meta, $null))
$out = [IO.File]::Create($Path)
try { $enc.Save($out) } finally { $out.Close() }
"ok"
