# Drags files onto Filezy the way Explorer does: a real OLE drag that starts in
# this process with a real mouse button press, so Filezy's own drop target,
# cursor feedback and drop all run as they do for a user.
#
#   drag.ps1 -ProcId <pid> -Files "C:\a.txt|C:\b.txt" -Points "x,y[,ms[,keys]];..." [-Keys ctrl,shift,alt] [-Right] [-Cancel]
#
# Points are CSS pixels in Filezy's client area; ms is how long to rest there
# (default 60), and keys ("ctrl+shift", or "none") the modifiers held from that
# point on, to change them mid-drag. -Keys are the ones held from the start.
# The drag drops at the last point, or cancels there with -Cancel.
#
# How: Windows only feeds a drag loop mouse input when its thread is in the
# foreground and a button went down on one of its windows. So a tiny window of
# ours sits under the first point, the left (or right) button is pressed on it,
# and DoDragDrop runs while the pointer is moved along the path. Ctrl, Shift
# and Alt are set in this thread's key state, which is what OLE reads, so no
# other window sees those keys.
#
# Safety: the button is always released, before the drop so a right-drag menu
# can't catch the release, and the drag is cancelled instead of dropped if the
# window under the last point isn't Filezy's. The pointer goes back where it
# was. A drag never runs longer than 20 seconds.
#
# Prints JSON: hr (DoDragDrop's result), effect (what the drop reported back),
# performed and logical (the Performed/Logical Performed DropEffect formats the
# target set, -1 if none), and feedback (each effect the cursor showed, in order).
param(
    [Parameter(Mandatory)][int]$ProcId,
    [Parameter(Mandatory)][string]$Files,
    [Parameter(Mandatory)][string]$Points,
    [string]$Keys = "",
    [switch]$Right,
    [switch]$Cancel,
    [int]$Allowed = 7
)

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

[ComImport, Guid("00000121-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IDropSource {
    [PreserveSig] int QueryContinueDrag([MarshalAs(UnmanagedType.Bool)] bool fEscapePressed, int grfKeyState);
    [PreserveSig] int GiveFeedback(int dwEffect);
}

public class PathSource : IDropSource {
    public List<int[]> Points;
    public byte[] KeyState;
    // Key state to switch to on reaching each point (null: keep the current one).
    public List<byte[]> PointKeys;
    public bool Cancel;
    public bool Right;
    public uint Pid;
    public IntPtr Grip;
    public List<int> Feedback = new List<int>();
    // Where the pointer should be now. The drag loop only turns on input, so a
    // timer nudges the pointer around this spot by a pixel; that also keeps
    // DragOver coming while the drag rests in one place.
    volatile int x, y;
    int index;
    long until;
    bool released;
    Stopwatch clock = Stopwatch.StartNew();
    System.Threading.Timer pump;

    public void Start() {
        x = Points[0][0];
        y = Points[0][1];
        bool flip = false;
        pump = new System.Threading.Timer(_ => { flip = !flip; Drag.SetCursorPos(x + (flip ? 1 : 0), y); }, null, 0, 25);
    }

    public void Stop() {
        if (pump != null) pump.Dispose();
        Release();
    }

    public void Release() {
        if (released) return;
        released = true;
        // Hold the pointer still from here on (a right-drag's menu opens under it).
        if (pump != null) pump.Dispose();
        Drag.Button(Right, false);
        // Let Windows take the release before anything else happens.
        for (int i = 0; i < 50 && Drag.ButtonDown(Right); i++) System.Threading.Thread.Sleep(10);
    }

    public int QueryContinueDrag(bool esc, int keys) {
        // OLE reads the key state on every turn; keep ours in place.
        Drag.SetKeyboardState(KeyState);
        if (index == 0) Drag.ShowWindow(Grip, 0); // out of the way of the hit-testing
        if (clock.ElapsedMilliseconds > 20000) { Release(); return 0x00040101; }
        if (clock.ElapsedMilliseconds < until) return 0;
        if (index < Points.Count) {
            if (PointKeys[index] != null) KeyState = PointKeys[index];
            int[] p = Points[index++];
            x = p[0];
            y = p[1];
            until = clock.ElapsedMilliseconds + p[2];
            Drag.SetKeyboardState(KeyState);
            Drag.SetCursorPos(x, y);
            return 0;
        }
        bool ours = Drag.PidAt(Points[Points.Count - 1]) == Pid;
        Release();
        return (Cancel || !ours) ? 0x00040101 : 0x00040100; // DRAGDROP_S_CANCEL : DRAGDROP_S_DROP
    }

    public int GiveFeedback(int effect) {
        if (Feedback.Count == 0 || Feedback[Feedback.Count - 1] != effect) Feedback.Add(effect);
        return 0x00040102; // DRAGDROP_S_USEDEFAULTCURSORS
    }
}

public static class Drag {
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam, lParam; public uint time; public POINT pt; }
    [DllImport("ole32.dll")] static extern int OleInitialize(IntPtr p);
    [DllImport("ole32.dll")] static extern int DoDragDrop(IDataObject data, IDropSource src, int ok, out int effect);
    [DllImport("ole32.dll")] static extern void ReleaseStgMedium(ref STGMEDIUM m);
    [DllImport("shell32.dll")] static extern int SHCreateDataObject(IntPtr folder, uint cidl, IntPtr apidl, IDataObject inner, ref Guid riid, out IDataObject obj);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern bool SetKeyboardState(byte[] state);
    [DllImport("user32.dll")] static extern bool GetKeyboardState(byte[] state);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vk);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint x, uint y, uint data, IntPtr extra);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr CreateWindowExW(uint ex, string cls, string name, uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] static extern bool DestroyWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] static extern bool PeekMessageW(out MSG m, IntPtr h, uint min, uint max, uint remove);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG m);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessageW(ref MSG m);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern uint RegisterClipboardFormatW(string name);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalAlloc(uint flags, UIntPtr size);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalLock(IntPtr h);
    [DllImport("kernel32.dll")] static extern bool GlobalUnlock(IntPtr h);

    public static void Button(bool right, bool down) {
        mouse_event(right ? (down ? 0x0008u : 0x0010u) : (down ? 0x0002u : 0x0004u), 0, 0, 0, IntPtr.Zero);
    }

    public static bool ButtonDown(bool right) {
        return (GetAsyncKeyState(right ? 0x02 : 0x01) & 0x8000) != 0;
    }

    /// The process owning the top-level window at a point. (The window right
    /// under it can be WebView2's, owned by its browser process.)
    public static uint PidAt(int[] p) {
        POINT pt; pt.X = p[0]; pt.Y = p[1];
        uint pid; GetWindowThreadProcessId(GetAncestor(WindowFromPoint(pt), 2), out pid);
        return pid;
    }

    static void Pump(int ms) {
        var clock = Stopwatch.StartNew();
        do {
            MSG m;
            while (PeekMessageW(out m, IntPtr.Zero, 0, 0, 1)) { TranslateMessage(ref m); DispatchMessageW(ref m); }
            System.Threading.Thread.Sleep(5);
        } while (clock.ElapsedMilliseconds < ms);
    }

    static IntPtr MainWindow(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256);
            if (p == pid && IsWindowVisible(h) && sb.ToString() == "Filezy") { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    static IDataObject Files(string[] files) {
        Guid iid = new Guid("0000010e-0000-0000-C000-000000000046");
        IDataObject data;
        Marshal.ThrowExceptionForHR(SHCreateDataObject(IntPtr.Zero, 0, IntPtr.Zero, null, ref iid, out data));
        var sb = new StringBuilder();
        foreach (var f in files) { sb.Append(f); sb.Append('\0'); }
        sb.Append('\0');
        byte[] names = Encoding.Unicode.GetBytes(sb.ToString());
        // DROPFILES: offset of the names, point, fNC, fWide; then the names.
        IntPtr h = GlobalAlloc(0x0042, (UIntPtr)(20 + names.Length));
        IntPtr ptr = GlobalLock(h);
        Marshal.WriteInt32(ptr, 0, 20);
        Marshal.WriteInt32(ptr, 16, 1);
        Marshal.Copy(names, 0, ptr + 20, names.Length);
        GlobalUnlock(h);
        var fmt = new FORMATETC { cfFormat = 15, dwAspect = DVASPECT.DVASPECT_CONTENT, lindex = -1, tymed = TYMED.TYMED_HGLOBAL };
        var med = new STGMEDIUM { tymed = TYMED.TYMED_HGLOBAL, unionmember = h };
        data.SetData(ref fmt, ref med, true);
        return data;
    }

    static int Dword(IDataObject data, string format) {
        var fmt = new FORMATETC { cfFormat = (short)RegisterClipboardFormatW(format), dwAspect = DVASPECT.DVASPECT_CONTENT, lindex = -1, tymed = TYMED.TYMED_HGLOBAL };
        try {
            STGMEDIUM med;
            data.GetData(ref fmt, out med);
            int v = Marshal.ReadInt32(GlobalLock(med.unionmember));
            GlobalUnlock(med.unionmember);
            ReleaseStgMedium(ref med);
            return v;
        } catch { return -1; }
    }

    public static string Run(uint pid, string files, string points, string keys, bool right, bool cancel, int allowed) {
        SetProcessDpiAwarenessContext(new IntPtr(-4)); // per-monitor v2: physical pixels throughout
        OleInitialize(IntPtr.Zero);
        IntPtr win = MainWindow(pid);
        if (win == IntPtr.Zero) throw new Exception("no Filezy window for process " + pid);
        double scale = GetDpiForWindow(win) / 96.0;
        var basis = new byte[256];
        GetKeyboardState(basis);
        // A key state with the button held and these modifiers ("ctrl", "shift", "alt").
        Func<IEnumerable<string>, byte[]> stateWith = names => {
            var st = (byte[])basis.Clone();
            st[0x10] = st[0x11] = st[0x12] = 0;
            foreach (var k in names) {
                if (k == "ctrl") st[0x11] = 0x80;
                if (k == "shift") st[0x10] = 0x80;
                if (k == "alt") st[0x12] = 0x80;
            }
            st[right ? 0x02 : 0x01] = 0x80;
            return st;
        };
        var path = new List<int[]>();
        var pointKeys = new List<byte[]>();
        foreach (var part in points.Split(';')) {
            var v = part.Split(',');
            POINT p; p.X = (int)Math.Round(double.Parse(v[0]) * scale); p.Y = (int)Math.Round(double.Parse(v[1]) * scale);
            ClientToScreen(win, ref p);
            path.Add(new int[] { p.X, p.Y, v.Length > 2 ? int.Parse(v[2]) : 60 });
            pointKeys.Add(v.Length > 3 ? stateWith(v[3].ToLowerInvariant().Split('+')) : null);
        }
        var state = stateWith(keys.ToLowerInvariant().Split(','));
        IDataObject data = Files(files.Split('|'));

        POINT home; GetCursorPos(out home);
        // The grip: a small topmost window of ours under the first point.
        int[] start = path[0];
        const uint WS_POPUP = 0x80000000, WS_VISIBLE = 0x10000000, WS_EX_TOPMOST = 0x8, WS_EX_TOOLWINDOW = 0x80;
        IntPtr grip = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, "STATIC", "", WS_POPUP | WS_VISIBLE, start[0] - 6, start[1] - 6, 12, 12, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        var src = new PathSource { Points = path, PointKeys = pointKeys, KeyState = state, Cancel = cancel, Right = right, Pid = pid, Grip = grip };
        int hr = -1, effect = 0;
        try {
            SetCursorPos(start[0], start[1]);
            Pump(150);
            Button(right, true);
            Pump(100);
            src.Start();
            hr = DoDragDrop(data, src, allowed, out effect);
        } finally {
            src.Stop();
            DestroyWindow(grip);
            SetCursorPos(home.X, home.Y);
        }
        return string.Format("{{\"hr\":{0},\"effect\":{1},\"performed\":{2},\"logical\":{3},\"feedback\":[{4}]}}",
            hr, effect, Dword(data, "Performed DropEffect"), Dword(data, "Logical Performed DropEffect"),
            string.Join(",", src.Feedback));
    }
}
"@

[Drag]::Run([uint32]$ProcId, $Files, $Points, $Keys, [bool]$Right, [bool]$Cancel, $Allowed)
