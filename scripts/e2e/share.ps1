# Finds Windows' Share sheet, a system window that lives outside the app's
# process, and prints what it says it is sharing ("You are sharing x.txt").
# -Close dismisses it with its own Close button.
#
# Only that one line is printed: the sheet also lists the user's contacts and
# accounts, which must never end up in test output.
param(
    [int]$Timeout = 15,
    [switch]$Close
)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
function Prop($p, $v) { New-Object System.Windows.Automation.PropertyCondition($p, $v) }

$deadline = (Get-Date).AddSeconds($Timeout)
while ($true) {
    foreach ($frame in $A::RootElement.FindAll($TS::Children, (Prop $A::ClassNameProperty "ApplicationFrameWindow"))) {
        $sheet = $frame.FindFirst($TS::Children, (Prop $A::NameProperty "Share"))
        if (-not $sheet) { continue }
        $what = $sheet.FindFirst($TS::Descendants, (Prop $A::ClassNameProperty "SharePicker"))
        # The sheet fills its text in after it appears.
        if (-not $what -or -not $what.Current.Name) { continue }
        if ($Close) {
            $btn = $sheet.FindFirst($TS::Descendants, (Prop $A::AutomationIdProperty "SharePickerCloseButton"))
            if (-not $btn) { throw "the Share sheet has no Close button" }
            $btn.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
            "closed"
            exit 0
        }
        $what.Current.Name
        exit 0
    }
    if ((Get-Date) -gt $deadline) { throw "no Share sheet within $Timeout s" }
    Start-Sleep -Milliseconds 200
}
