# Reports the preview pane's native host window (src-tauri/src/preview.rs) in
# a Filezy process. Read-only: it sends no input and moves nothing.
#
#   preview.ps1 -ProcId <pid> [-Shot <file.png>]
#   preview.ps1 -ProcId <pid> -Rect x,y,w,h
#
# Prints JSON: visible; owned, whether Filezy's main window owns the host;
# children, the class and process of each window inside the host (a preview
# handler draws from prevhost.exe or its own server); and ink, the share of the
# host's pixels that differ from its most common colour. The pixels come from
# PrintWindow, which captures the window even when others cover it, but not a
# previewer that draws through WebView2 (Edge's PDF one). When PrintWindow
# sees nothing, the screen is read instead, if nothing covers the host there.
# inkFrom says which: "window", "screen", or "covered" (no reading possible).
# -Shot saves the pixels that were measured as a PNG.
#
# With -Rect (physical pixels of Filezy's client area) it measures that part of
# Filezy's own window instead, from the screen, and prints {ink, inkFrom}:
# for what WebView2 draws there, such as its PDF viewer.
param(
    [Parameter(Mandatory)][int]$ProcId,
    [string]$Shot = "",
    [string]$Rect = ""
)

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;

public static class PreviewHost {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);

    /// The share of pixels that differ from the most common colour.
    static double Ink(Bitmap b) {
        var counts = new Dictionary<int, int>();
        int total = 0, top = 0;
        for (int y = 0; y < b.Height; y += 2)
            for (int x = 0; x < b.Width; x += 2) {
                // Quantized, so antialiasing noise in a flat area doesn't count as ink.
                int c = b.GetPixel(x, y).ToArgb() & 0xF0F0F0;
                int n; counts.TryGetValue(c, out n); counts[c] = ++n;
                if (n > top) top = n;
                total++;
            }
        return total == 0 ? 0 : 1.0 - (double)top / total;
    }

    /// Whether the host is what's on screen at a grid of points over it.
    static bool OnTop(IntPtr host, RECT r) {
        for (int i = 1; i < 4; i++)
            for (int j = 1; j < 4; j++) {
                POINT p; p.X = r.L + (r.R - r.L) * i / 4; p.Y = r.T + (r.B - r.T) * j / 4;
                if (GetAncestor(WindowFromPoint(p), 2) != host) return false; // GA_ROOT
            }
        return true;
    }

    static string Class(IntPtr h) { var s = new StringBuilder(256); GetClassNameW(h, s, 256); return s.ToString(); }

    static string Esc(string s) { return s.Replace("\\", "\\\\").Replace("\"", "\\\""); }

    public static string Report(uint pid, string shot, string rect) {
        SetProcessDpiAwarenessContext(new IntPtr(-4)); // physical pixels throughout
        IntPtr main = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            var t = new StringBuilder(256); GetWindowTextW(h, t, 256);
            if (p == pid && IsWindowVisible(h) && t.ToString() == "Filezy") { main = h; return false; }
            return true;
        }, IntPtr.Zero);
        if (main == IntPtr.Zero) throw new Exception("no Filezy window for process " + pid);
        if (rect != "") {
            var v = Array.ConvertAll(rect.Split(','), int.Parse);
            POINT o; o.X = v[0]; o.Y = v[1];
            ClientToScreen(main, ref o);
            RECT s; s.L = o.X; s.T = o.Y; s.R = o.X + v[2]; s.B = o.Y + v[3];
            if (!OnTop(main, s)) return "{\"ink\":0,\"inkFrom\":\"covered\"}";
            using (var b = new Bitmap(v[2], v[3], PixelFormat.Format32bppArgb)) {
                using (var g = Graphics.FromImage(b)) g.CopyFromScreen(s.L, s.T, 0, 0, b.Size);
                return string.Format(System.Globalization.CultureInfo.InvariantCulture, "{{\"ink\":{0:0.####},\"inkFrom\":\"screen\"}}", Ink(b));
            }
        }
        // An owned popup, so a top-level window of the same process.
        IntPtr host = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && Class(h) == "FilezyPreviewHost") { host = h; return false; }
            return true;
        }, IntPtr.Zero);
        if (host == IntPtr.Zero) return "{\"visible\":false,\"owned\":false,\"children\":[],\"ink\":0}";
        bool owned = GetWindow(host, 4) == main; // GW_OWNER
        var kids = new List<string>();
        EnumChildWindows(host, (h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            string name = "?";
            try { name = Process.GetProcessById((int)p).ProcessName; } catch { }
            kids.Add(string.Format("{{\"class\":\"{0}\",\"process\":\"{1}\"}}", Esc(Class(h)), Esc(name)));
            return true;
        }, IntPtr.Zero);
        bool visible = IsWindowVisible(host);
        double ink = 0;
        string from = "window";
        RECT r;
        GetWindowRect(host, out r);
        if (visible && r.R > r.L && r.B > r.T) {
            using (var part = new Bitmap(r.R - r.L, r.B - r.T, PixelFormat.Format32bppArgb)) {
                using (var g = Graphics.FromImage(part)) {
                    IntPtr dc = g.GetHdc();
                    PrintWindow(host, dc, 2); // PW_RENDERFULLCONTENT
                    g.ReleaseHdc(dc);
                }
                ink = Ink(part);
                if (ink < 0.002) {
                    from = OnTop(host, r) ? "screen" : "covered";
                    if (from == "screen")
                        using (var g = Graphics.FromImage(part)) g.CopyFromScreen(r.L, r.T, 0, 0, part.Size);
                    ink = from == "screen" ? Ink(part) : 0;
                }
                if (shot != "") part.Save(shot, ImageFormat.Png);
            }
        }
        return string.Format(System.Globalization.CultureInfo.InvariantCulture,
            "{{\"visible\":{0},\"owned\":{1},\"children\":[{2}],\"ink\":{3:0.####},\"inkFrom\":\"{4}\"}}",
            visible ? "true" : "false", owned ? "true" : "false", string.Join(",", kids), ink, from);
    }
}
"@

[PreviewHost]::Report([uint32]$ProcId, $Shot, $Rect)
