# A real mouse drag inside Filezy: press at the first point, move along the
# path, release at the last. For drags that start in Filezy itself, which only
# real input can start.
#
#   mouse.ps1 -ProcId <pid> -Points "x,y[,ms];x,y[,ms]" [-Ctrl] [-Right]
#
# Points are CSS pixels in Filezy's client area; ms is how long to rest there
# (default 60). -Ctrl holds Ctrl from the press to the release.
#
# Safety: nothing happens unless Filezy is in the foreground and under the
# first point. If the last point isn't over Filezy, Escape cancels the drag
# instead of a release dropping it elsewhere. The button and Ctrl are always
# released, and the pointer goes back where it was.
param(
    [Parameter(Mandatory)][int]$ProcId,
    [Parameter(Mandatory)][string]$Points,
    [switch]$Ctrl,
    [switch]$Right
)

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class Mouse {
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint x, uint y, uint data, IntPtr extra);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);

    static uint PidOf(IntPtr h) { uint pid; GetWindowThreadProcessId(h, out pid); return pid; }
    static uint PidAt(POINT p) { return PidOf(GetAncestor(WindowFromPoint(p), 2)); }

    static IntPtr MainWindow(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256);
            if (PidOf(h) == pid && IsWindowVisible(h) && sb.ToString() == "Filezy") { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    static void Rest(POINT p, int ms) {
        // A still pointer gives a drag loop no input; nudge it by a pixel.
        for (int t = 0; t < ms; t += 25) {
            SetCursorPos(p.X + ((t / 25) % 2), p.Y);
            Thread.Sleep(25);
        }
        SetCursorPos(p.X, p.Y);
    }

    public static string Run(uint pid, string points, bool ctrl, bool right) {
        SetProcessDpiAwarenessContext(new IntPtr(-4));
        IntPtr win = MainWindow(pid);
        if (win == IntPtr.Zero) throw new Exception("no Filezy window for process " + pid);
        double scale = GetDpiForWindow(win) / 96.0;
        var path = new List<POINT>();
        var rest = new List<int>();
        foreach (var part in points.Split(';')) {
            var v = part.Split(',');
            POINT p; p.X = (int)Math.Round(double.Parse(v[0]) * scale); p.Y = (int)Math.Round(double.Parse(v[1]) * scale);
            ClientToScreen(win, ref p);
            path.Add(p);
            rest.Add(v.Length > 2 ? int.Parse(v[2]) : 60);
        }
        if (PidOf(GetForegroundWindow()) != pid) throw new Exception("Filezy isn't in the foreground");
        if (PidAt(path[0]) != pid) throw new Exception("Filezy isn't under the first point");
        POINT home; GetCursorPos(out home);
        uint down = right ? 0x0008u : 0x0002u, up = right ? 0x0010u : 0x0004u;
        bool pressed = false, held = false;
        string outcome = "dropped";
        try {
            SetCursorPos(path[0].X, path[0].Y);
            Thread.Sleep(100);
            if (ctrl) { keybd_event(0x11, 0, 0, IntPtr.Zero); held = true; }
            mouse_event(down, 0, 0, 0, IntPtr.Zero);
            pressed = true;
            Rest(path[0], rest[0]);
            for (int i = 1; i < path.Count; i++) {
                POINT a = path[i - 1], b = path[i];
                // Small steps, as a hand moves: the page starts the drag on movement.
                for (int s = 1; s <= 12; s++) {
                    SetCursorPos(a.X + (b.X - a.X) * s / 12, a.Y + (b.Y - a.Y) * s / 12);
                    Thread.Sleep(15);
                }
                Rest(b, rest[i]);
            }
            if (PidAt(path[path.Count - 1]) != pid) {
                keybd_event(0x1B, 0, 0, IntPtr.Zero);
                keybd_event(0x1B, 0, 2, IntPtr.Zero);
                Thread.Sleep(100);
                outcome = "cancelled: the last point isn't over Filezy";
            }
        } finally {
            if (pressed) mouse_event(up, 0, 0, 0, IntPtr.Zero);
            if (held) keybd_event(0x11, 0, 2, IntPtr.Zero);
            Thread.Sleep(100);
            SetCursorPos(home.X, home.Y);
        }
        return outcome;
    }
}
"@

[Mouse]::Run([uint32]$ProcId, $Points, [bool]$Ctrl, [bool]$Right)
