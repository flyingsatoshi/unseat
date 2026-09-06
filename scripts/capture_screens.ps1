Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class UnseatNative5 {
  [DllImport("user32.dll")]
  public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern IntPtr FindWindowW(string lpClassName, IntPtr lpWindowName);
  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
  [DllImport("user32.dll")]
  public static extern bool SetWindowPos(IntPtr hWnd, IntPtr insertAfter, int X, int Y, int cx, int cy, uint flags);
  [DllImport("user32.dll")]
  public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")]
  public static extern IntPtr SendMessageW(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll")]
  public static extern bool PostMessageW(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
  public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[void][UnseatNative5]::SetProcessDpiAwarenessContext([IntPtr](-4))
Add-Type -AssemblyName System.Drawing

function Save-Shot([IntPtr]$hwnd, [string]$path, [int]$pad) {
  $r = New-Object UnseatNative5+RECT
  [void][UnseatNative5]::GetWindowRect($hwnd, [ref]$r)
  $w = $r.Right - $r.Left
  $h = $r.Bottom - $r.Top
  if ($w -lt 8 -or $h -lt 8) { throw "empty window $path" }
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.Left, $r.Top, 0, 0, (New-Object System.Drawing.Size $w, $h))
  $g.Dispose()
  $out = New-Object System.Drawing.Bitmap ($w + 2 * $pad), ($h + 2 * $pad)
  $og = [System.Drawing.Graphics]::FromImage($out)
  $og.Clear([System.Drawing.Color]::FromArgb(255, 18, 18, 20))
  $og.DrawImage($bmp, $pad, $pad, $w, $h)
  $og.Dispose()
  $bmp.Dispose()
  $out.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $out.Dispose()
  "wrote $path ${w}x${h} pad=$pad"
}

$shot = "D:\Projects\unseat\assets\screenshots"
$zero = [IntPtr]::Zero
$HWND_TOPMOST = [IntPtr](-1)

$settings = [UnseatNative5]::FindWindowW("UnseatSettings", $zero)
if ($settings -ne $zero) {
  [void][UnseatNative5]::PostMessageW($settings, 0x0010, $zero, $zero) # WM_CLOSE
  Start-Sleep -Milliseconds 400
}

$widget = [UnseatNative5]::FindWindowW("UnseatWidget", $zero)
if ($widget -eq $zero) { throw "UnseatWidget missing" }
[void][UnseatNative5]::ShowWindow($widget, 4)
[void][UnseatNative5]::SetWindowPos($widget, $HWND_TOPMOST, 200, 200, 0, 0, 0x0041)
Start-Sleep -Milliseconds 500
Save-Shot $widget (Join-Path $shot "widget.png") 56

$hidden = [UnseatNative5]::FindWindowW("UnseatHidden", $zero)
if ($hidden -ne $zero) {
  [void][UnseatNative5]::SendMessageW($hidden, 0x0111, [IntPtr]12, $zero)
  Start-Sleep -Milliseconds 700
}
$settings = [UnseatNative5]::FindWindowW("UnseatSettings", $zero)
if ($settings -eq $zero) { throw "UnseatSettings missing" }
[void][UnseatNative5]::SetWindowPos($settings, $HWND_TOPMOST, 80, 40, 0, 0, 0x0041)
Start-Sleep -Milliseconds 500
Save-Shot $settings (Join-Path $shot "settings.png") 18
Get-Item (Join-Path $shot "*.png") | Select-Object Name, Length, LastWriteTime
