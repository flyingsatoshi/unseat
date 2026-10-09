# Renders the Unseat "Rise" icon (a figure stepping up off a seat) and writes:
#   assets/unseat.ico          16-256 px, PNG entries; 16-24 px use the hand-tuned small master
#   assets/icon.svg            256 px master
#   assets/icon-small.svg      16 px master for tray and small taskbar sizes
#   assets/icon.png, logo.png  512 px; assets/unseat.png 1024 px
# Usage: powershell -ExecutionPolicy Bypass -File scripts/make_icon.ps1
param([string]$Root = (Split-Path -Parent $PSScriptRoot))

$ink = "#14181F"; $mint = "#3DD68C"; $white = "#FFFFFF"

# Geometry in each master's own units: (tile size, corner radius, seat x y w h, body x top bottom width, head cx cy r)
$big = @{ Size = 256; Inset = 8; Radius = 58; Seat = @(46, 170, 96, 24); Body = @(162, 116, 196, 30); Head = @(162, 74, 22) }
$small = @{ Size = 16; Inset = 0; Radius = 3.6; Seat = @(2.4, 10.6, 6, 2.2); Body = @(10.4, 7.4, 12.4, 2.4); Head = @(10.4, 4.4, 1.7) }

function Svg($m) {
  $s = $m.Size; $i = $m.Inset; $t = $s - 2 * $i
  $seat = $m.Seat; $b = $m.Body; $h = $m.Head
  @"
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 $s $s" width="$s" height="$s">
<rect x="$i" y="$i" width="$t" height="$t" rx="$($m.Radius)" fill="$ink"/>
<rect x="$($seat[0])" y="$($seat[1])" width="$($seat[2])" height="$($seat[3])" rx="$($seat[3] / 2)" fill="$mint"/>
<path d="M$($b[0]) $($b[2])V$($b[1])" stroke="$white" stroke-width="$($b[3])" stroke-linecap="round"/>
<circle cx="$($h[0])" cy="$($h[1])" r="$($h[2])" fill="$white"/>
</svg>
"@
}

$assets = Join-Path $Root "assets"
[IO.File]::WriteAllText((Join-Path $assets "icon.svg"), (Svg $big))
[IO.File]::WriteAllText((Join-Path $assets "icon-small.svg"), (Svg $small))

Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Drawing.dll -TypeDefinition @"
using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;

public static class UnseatIcon {
  static GraphicsPath RoundRect(float x, float y, float w, float h, float r) {
    var p = new GraphicsPath();
    float d = r * 2;
    p.AddArc(x, y, d, d, 180, 90);
    p.AddArc(x + w - d, y, d, d, 270, 90);
    p.AddArc(x + w - d, y + h - d, d, d, 0, 90);
    p.AddArc(x, y + h - d, d, d, 90, 90);
    p.CloseFigure();
    return p;
  }

  // m: size, inset, radius, seat x,y,w,h, body x,top,bottom,width, head cx,cy,r (master units)
  public static Bitmap Render(int px, float[] m) {
    int ss = px <= 64 ? 8 : 4;
    float k = (float)(px * ss) / m[0];
    using (var big = new Bitmap(px * ss, px * ss, PixelFormat.Format32bppArgb)) {
      using (var g = Graphics.FromImage(big)) {
        g.Clear(Color.Transparent);
        g.SmoothingMode = SmoothingMode.AntiAlias;
        g.PixelOffsetMode = PixelOffsetMode.HighQuality;
        float inset = m[1] * k, side = (m[0] - 2 * m[1]) * k;
        using (var tile = RoundRect(inset, inset, side, side, m[2] * k))
        using (var ink = new SolidBrush(ColorTranslator.FromHtml("#14181F")))
          g.FillPath(ink, tile);
        using (var seat = RoundRect(m[3] * k, m[4] * k, m[5] * k, m[6] * k, m[6] * k / 2))
        using (var mint = new SolidBrush(ColorTranslator.FromHtml("#3DD68C")))
          g.FillPath(mint, seat);
        using (var pen = new Pen(Color.White, m[10] * k)) {
          pen.StartCap = LineCap.Round; pen.EndCap = LineCap.Round;
          g.DrawLine(pen, m[7] * k, m[9] * k, m[7] * k, m[8] * k);
        }
        using (var white = new SolidBrush(Color.White))
          g.FillEllipse(white, (m[11] - m[13]) * k, (m[12] - m[13]) * k, 2 * m[13] * k, 2 * m[13] * k);
      }
      var outBmp = new Bitmap(px, px, PixelFormat.Format32bppArgb);
      using (var g = Graphics.FromImage(outBmp)) {
        g.Clear(Color.Transparent);
        g.InterpolationMode = InterpolationMode.HighQualityBicubic;
        g.PixelOffsetMode = PixelOffsetMode.HighQuality;
        g.CompositingMode = CompositingMode.SourceCopy;
        g.DrawImage(big, new Rectangle(0, 0, px, px));
      }
      return outBmp;
    }
  }
}
"@

function Floats($m) {
  [float[]](@($m.Size, $m.Inset, $m.Radius) + $m.Seat + $m.Body + $m.Head)
}
$bigF = Floats $big; $smallF = Floats $small
function Master([int]$px) { if ($px -le 24) { $smallF } else { $bigF } }

$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$pngs = @()
foreach ($size in $sizes) {
  $bmp = [UnseatIcon]::Render($size, (Master $size))
  $ms = New-Object System.IO.MemoryStream
  $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  $pngs += , $ms.ToArray()
}

# ICO container with PNG-compressed entries (supported since Windows Vista).
$ico = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $ico
$w.Write([UInt16]0); $w.Write([UInt16]1); $w.Write([UInt16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $dim = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
  $w.Write([byte]$dim); $w.Write([byte]$dim); $w.Write([byte]0); $w.Write([byte]0)
  $w.Write([UInt16]1); $w.Write([UInt16]32)
  $w.Write([UInt32]$pngs[$i].Length); $w.Write([UInt32]$offset)
  $offset += $pngs[$i].Length
}
foreach ($png in $pngs) { $w.Write($png) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $assets "unseat.ico"), $ico.ToArray())

foreach ($pair in @(@("icon.png", 512), @("logo.png", 512), @("unseat.png", 1024))) {
  $bmp = [UnseatIcon]::Render($pair[1], $bigF)
  $bmp.Save((Join-Path $assets $pair[0]), [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
}

# Preview: every ICO size at 1:1 on dark and light, like a taskbar.
$sheet = New-Object System.Drawing.Bitmap 760, 220
$g = [System.Drawing.Graphics]::FromImage($sheet)
$g.Clear([System.Drawing.Color]::FromArgb(255, 32, 32, 34))
$g.FillRectangle((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 238, 238, 238))), 0, 110, 760, 110)
$x = 12
foreach ($size in $sizes) {
  $bmp = [UnseatIcon]::Render($size, (Master $size))
  $draw = [Math]::Min($size, 96)
  $g.DrawImage($bmp, $x, [int](55 - $draw / 2), $draw, $draw)
  $g.DrawImage($bmp, $x, [int](165 - $draw / 2), $draw, $draw)
  $x += $draw + 14
  $bmp.Dispose()
}
$g.Dispose()
$preview = Join-Path $Root "target\icon-preview.png"
$sheet.Save($preview, [System.Drawing.Imaging.ImageFormat]::Png)
$sheet.Dispose()
"wrote unseat.ico ($($sizes -join ', ')), icon.svg, icon-small.svg, PNGs, $preview"
