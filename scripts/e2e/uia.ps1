# Drives native Windows dialogs (copy conflicts, progress, confirmations) that the
# WebView cannot reach, through UI Automation.
#
#   uia.ps1 -Window "Replace or Skip*" -Click "Skip this file"
#   uia.ps1 -Window "*complete*" -Dump          # print the dialog's element tree
#   uia.ps1 -Window "Replace or Skip*" -Close    # close it, which cancels the operation
#   uia.ps1 -Window "Delete File" -Command 6       # press the dialog button with id 6 (Yes)
#   uia.ps1 -Menu -Key 0x43                        # press C (an access key) in an open popup menu
#   uia.ps1 -Menu -Key 27                          # press Escape there
#   uia.ps1 -Menu -Count                           # print how many entries the open popup menu has
#   uia.ps1 -Menu -List                            # print its entries
#   uia.ps1 -Menu -Choose "Rename"                 # pick an entry by its text (arrow keys and Enter)
#
# Menu keys are posted to the menu's own window, whose menu loop reads them
# like typed keys; nothing goes through the real keyboard or to another window.
# A menu's items expose no UI Automation actions.
#
# -Window and -Click are -like patterns. -ProcId limits the search to one process.
# Windows are found with EnumWindows rather than the UIA tree, because a dialog
# with an owner window can sit under that owner in the UIA tree.
param(
    [string]$Window = "*",
    [switch]$Menu,
    [string]$Click,
    [int]$Timeout = 15,
    [int]$ProcId = 0,
    [switch]$Dump,
    [switch]$Close,
    [int]$Command = 0,
    [int]$Key = 0,
    [switch]$Count,
    [switch]$List,
    [string]$Choose
)

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class Wnd {
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern int GetWindowTextW(IntPtr h, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder s, int n);
    [DllImport("user32.dll")] static extern int GetClassNameW(IntPtr h, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder s, int n);
    public static string ClassOf(IntPtr h) { var sb = new StringBuilder(256); GetClassNameW(h, sb, 256); return sb.ToString(); }
    [DllImport("user32.dll")] static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern int GetMenuItemCount(IntPtr m);
    [DllImport("user32.dll")] static extern uint GetMenuState(IntPtr m, uint id, uint flags);
    [DllImport("user32.dll")] static extern int GetMenuStringW(IntPtr m, uint id, [MarshalAs(UnmanagedType.LPWStr)] StringBuilder s, int n, uint flags);
    // A popup menu window's menu (MN_GETHMENU); menus are readable from any process.
    public static IntPtr MenuOf(IntPtr w) { return SendMessageW(w, 0x01E1, IntPtr.Zero, IntPtr.Zero); }
    public static int Count(IntPtr menu) { return GetMenuItemCount(menu); }
    public static string List(IntPtr menu) {
        var sb = new StringBuilder();
        for (uint i = 0; i < (uint)GetMenuItemCount(menu); i++) {
            var t = new StringBuilder(256); GetMenuStringW(menu, i, t, 256, 0x400);
            sb.AppendLine(i + " state=0x" + GetMenuState(menu, i, 0x400).ToString("x") + " '" + t + "'");
        }
        return sb.ToString();
    }
    /// The position of the entry named `text` (access-key marks and shortcut text ignored), or -1.
    public static int IndexOf(IntPtr menu, string text) {
        for (uint i = 0; i < (uint)GetMenuItemCount(menu); i++) {
            var sb = new StringBuilder(256); GetMenuStringW(menu, i, sb, 256, 0x400);
            if (sb.ToString().Replace("&", "").Split('\t')[0] == text) return (int)i;
        }
        return -1;
    }
    public static bool Hilited(IntPtr menu, int i) { return (GetMenuState(menu, (uint)i, 0x400) & 0x80) != 0; }
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint MapVirtualKeyW(uint code, uint type);
    /// Key `vk` down and up, posted to window `w` only.
    public static void Post(IntPtr w, byte vk) {
        long scan = (long)MapVirtualKeyW(vk, 0) << 16;
        PostMessageW(w, 0x100, (IntPtr)vk, (IntPtr)(scan | 1));
        PostMessageW(w, 0x101, (IntPtr)vk, (IntPtr)(scan | 1 | 0xC0000000L));
    }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int l, t, r, b; }
    [StructLayout(LayoutKind.Sequential)] struct GUITHREADINFO {
        public int cbSize; public uint flags; public IntPtr active, focus, capture, menuOwner, moveSize, caret; public RECT rc;
    }
    [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint tid, ref GUITHREADINFO gi);
    // Who has the keyboard: the foreground window, its thread's focus and
    // whether that thread is in a menu loop (GUI_INMENUMODE 4, GUI_POPUPMENUMODE 16).
    public static string Keyboard() {
        var fg = GetForegroundWindow();
        uint pid; uint tid = GetWindowThreadProcessId(fg, out pid);
        var gi = new GUITHREADINFO(); gi.cbSize = Marshal.SizeOf(gi);
        GetGUIThreadInfo(tid, ref gi);
        uint fpid; GetWindowThreadProcessId(gi.focus, out fpid);
        return string.Format("foreground {0} '{1}' pid {2}; focus {3} pid {4}; menu owner {5}; flags {6:x}",
            ClassOf(fg), Title(fg), pid, ClassOf(gi.focus), fpid, ClassOf(gi.menuOwner), gi.flags);
    }
    static string Title(IntPtr h) { var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256); return sb.ToString(); }
    public static List<KeyValuePair<IntPtr, string>> List(uint onlyPid) {
        var all = new List<KeyValuePair<IntPtr, string>>();
        EnumWindows((h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (IsWindowVisible(h) && (onlyPid == 0 || pid == onlyPid)) {
                var sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
                all.Add(new KeyValuePair<IntPtr, string>(h, sb.ToString()));
            }
            return true;
        }, IntPtr.Zero);
        return all;
    }
}
"@
# Physical pixels for both UIA rectangles and the cursor.
[void][Wnd]::SetProcessDPIAware()
$A = [System.Windows.Automation.AutomationElement]
$TS = [System.Windows.Automation.TreeScope]
$Cond = [System.Windows.Automation.Condition]::TrueCondition

function Find-Window {
    foreach ($w in [Wnd]::List([uint32]$ProcId)) {
        # A popup menu is a titleless window of class #32768.
        if ($Menu) { if ([Wnd]::ClassOf($w.Key) -eq "#32768") { return $A::FromHandle($w.Key) } }
        elseif ($w.Value -like $Window) { return $A::FromHandle($w.Key) }
    }
    return $null
}

$deadline = (Get-Date).AddSeconds($Timeout)
$win = $null
while (-not $win) {
    $win = Find-Window
    if ($win) { break }
    if ((Get-Date) -gt $deadline) {
        $titles = ([Wnd]::List([uint32]$ProcId) | ForEach-Object { "'" + $_.Value + "'" }) -join ", "
        throw "no window like '$Window' within $Timeout s (visible: $titles)"
    }
    Start-Sleep -Milliseconds 100
}

if ($Dump) {
    foreach ($el in $win.FindAll($TS::Descendants, $Cond)) {
        $c = $el.Current
        "{0,-28} {1,-16} '{2}'" -f $c.ControlType.ProgrammaticName, $c.AutomationId, $c.Name
    }
    exit 0
}

if ($Count -or $Choose -or $List) {
    $w = [IntPtr]$win.Current.NativeWindowHandle
    $h = [Wnd]::MenuOf($w)
    if ($Count) { [Wnd]::Count($h); exit 0 }
    if ($List) { [Wnd]::List($h); exit 0 }
    $target = [Wnd]::IndexOf($h, $Choose)
    if ($target -lt 0) { throw "no menu entry '$Choose'" }
    # Down until the menu itself shows the entry highlighted, then Enter.
    for ($i = 0; $i -lt 2 * [Wnd]::Count($h) -and -not [Wnd]::Hilited($h, $target); $i++) {
        [Wnd]::Post($w, 0x28)
        Start-Sleep -Milliseconds 40
    }
    if (-not [Wnd]::Hilited($h, $target)) { throw "couldn't reach '$Choose' with the arrow keys (" + [Wnd]::Keyboard() + ")" }
    [Wnd]::Post($w, 0x0D)
    "chose '$Choose'"
    exit 0
}

if ($Key) {
    if (-not $Menu) { throw "-Key is for popup menus" }
    # A menu that has only just appeared may not be reading keys yet: press
    # again until it closes.
    for ($i = 0; $i -lt 20; $i++) {
        [Wnd]::Post([IntPtr]$win.Current.NativeWindowHandle, [byte]$Key)
        Start-Sleep -Milliseconds 250
        if (-not (Find-Window)) { "pressed"; exit 0 }
    }
    throw ("the menu stayed open (" + [Wnd]::Keyboard() + ")")
}

if ($Command) {
    # Message-box style dialogs draw their buttons as panes with no UIA actions,
    # but the panes keep their dialog command ids (the AutomationId in -Dump):
    # post WM_COMMAND to that window only, so no keystroke can land elsewhere.
    [void][Wnd]::PostMessageW([IntPtr]$win.Current.NativeWindowHandle, 0x0111, [IntPtr]$Command, [IntPtr]::Zero)
    "sent command $Command to '$($win.Current.Name)'"
    exit 0
}

if ($Close) {
    $wp = $win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
    $wp.Close()
    "closed '$($win.Current.Name)'"
    exit 0
}

if (-not $Click) { "found '$($win.Current.Name)'"; exit 0 }

while ($true) {
    $target = $null
    foreach ($el in $win.FindAll($TS::Descendants, $Cond)) {
        if ($el.Current.Name -like $Click -and $el.Current.IsEnabled) { $target = $el; break }
    }
    if ($target) { break }
    if ((Get-Date) -gt $deadline) { throw "no control like '$Click' in '$($win.Current.Name)'" }
    Start-Sleep -Milliseconds 100
}

$pattern = $null
if ($target.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
} elseif ($target.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
    $pattern.Select()
} else {
    # Some dialogs' buttons show up as plain panes with no actions, though they
    # are button windows (a progress dialog's Cancel is a CCPushButton). Those
    # take BM_CLICK: a press made of window messages, posted to that button only.
    $h = [IntPtr]$target.Current.NativeWindowHandle
    if ($h -eq [IntPtr]::Zero) { throw "'$($target.Current.Name)' has no invoke pattern and no window to press" }
    [void][Wnd]::PostMessageW($h, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)
}
"clicked '$($target.Current.Name)' in '$($win.Current.Name)'"
